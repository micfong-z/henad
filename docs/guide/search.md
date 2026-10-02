---
title: Searching a model
description: How to let Henad choose the configs to run, looking for the best value of an output or for every kind of behaviour a model shows.
icon: material/magnify
---

# Searching a model

A [sweep](sweeps.md) runs a list of configs fixed before the first run.
Some questions concern a few configs among a great many: the settings that give the largest epidemic, or the kinds of epidemic a model can produce at all.
A grid fine enough to answer them puts most of its runs far from the answer, and it grows with every parameter you add.

A **search** chooses its configs as it goes.
It runs a batch of configs, reads their results, and picks the next batch from the results so far.
Henad has four search methods.
A search runs from a spec file with a `[search]` table, on the same runs and seeds as a sweep, and writes the files of a sweep plus a few of its own.
The app runs the same searches from its [Sweep tab](app.md#search-mode), and plots them in its [Results tab](app.md#search).

Before we start, read the [parameter sweeps guide](sweeps.md) up to the end of [spec files](sweeps.md#spec-files).
A search uses the spec file's tables, replicates, seeds and reducers as a sweep does, and this page covers the parts a search adds.

## Sweep or search

A sweep suits a question about the whole space.
It gives a plot of an output against a parameter, a heatmap over two, or the design a sensitivity analysis needs.
Its design spreads the configs, and every part of the space gets its share.

A search suits a question about a small part of the space.
It spends most of its runs near its target, and it can take more parameters than a grid can cover.
Its configs bunch up in the places it looked.
A response plot or a sensitivity analysis drawn from them leans toward those places.

Pick the method by the question:

| Method | `algorithm` | Looks for | Suits |
|---|---|---|---|
| [Random search](#random-search) | `random` | The best value of an output | A baseline for the other methods |
| [Hill climbing](#hill-climbing) | `hill_climb` | The best value of an output | An output that changes smoothly, with few peaks |
| [Genetic algorithm](#genetic-algorithm) | `genetic` | The best value of an output | A noisy output, or parameters that act together |
| [Pattern Space Exploration](#pattern-space-exploration) | `pse` | Every pair of values two outputs can take | The regimes a model can produce |

## A first search

Let's find the SIR epidemic that peaks latest.
The infection rate, the recovery rate, the share of cells infected at the start and the tick of a second outbreak all move the peak, and they act on each other.
Here is a spec that searches all four with a genetic algorithm.
Henad's repository keeps it at `crates/henad-explore/specs/sir_search_genetic.toml`.

??? example "`sir_search_genetic.toml`"

    ``` toml
    --8<-- "crates/henad-explore/specs/sir_search_genetic.toml"
    ```

Save it as `sir_search_genetic.toml` and run it:

``` bash
cargo run --release -p henad-cli -- --spec sir_search_genetic.toml --out sir-genetic
```

The search prints its plan before the first run:

```text
search of SIR Epidemic (sir): genetic, 480 evaluations x 4 replicates = 1920 runs
  batch size:   32 candidates
  objective:    maximize the median of Infected:argmax
  search space: infection_rate from 0.05 to 0.9
                recovery_rate from 0.01 to 0.3
                initial_infected_pct from 0.001 to 0.05
                action.second_wave from 0 to 400
  search seed:  18174148693273609575
  layout:       14 lanes of 1 thread each
  memory:       112.0 KiB projected for concurrent runs
  series rows:  0
```

It ends with a line of counts and the best config it found:

```text
wrote 1920 runs to sir-genetic in 0.3s: 1920 ok, 0 non-finite, 0 failed
480/480 evaluations, best candidate 442: Infected:argmax 165, over 8 replicates
```

A **candidate** is one config the search chose.
An **evaluation** runs a candidate once for each replicate, here four times.
Candidate 442 won: over its runs, the median tick of the epidemic's peak is 165.
It has eight runs where the spec asks for four, for the reason given under [noise](#noise).

`--dry-run` prints the plan and stops, as it does for a sweep.

### The spec

We will go through the spec a part at a time.

``` toml title="sir_search_genetic.toml"
--8<-- "crates/henad-explore/specs/sir_search_genetic.toml:setup"
```

The top of the file is a sweep spec with no blocks.
`[set]` fixes a small grid, and `[run]` sets the length of each run and `replicates`, the runs of one evaluation.
A search reads its outputs from the reducers, and `[measure]` asks for the two this one needs, the peak and its tick.
`series_every = 0` writes no rows to `series.csv`, since the search reads only the reducers.
The `root` of `[seeds]` seeds the runs and the search.
The `[[action]]` table adds a second outbreak, and the search moves its tick.

``` toml
--8<-- "crates/henad-explore/specs/sir_search_genetic.toml:search"
```

`[search]` takes the place of the blocks, and a spec with both is refused.
`algorithm` names the method.
`max_evaluations` is the budget, and `batch_size` the most candidates run at once.
`objective` names the output that scores a candidate, as [objectives](#objectives) describes.
`space` lists the factors the search varies.

``` toml
--8<-- "crates/henad-explore/specs/sir_search_genetic.toml:genetic"
```

Each method but random search has a table of its own for its settings: `[search.hill_climb]`, `[search.genetic]` or `[search.pse]`.
A setting you leave out takes its default, and a table you leave out takes every default.
Pattern Space Exploration needs its table, for the two axes.
A table of another method is refused, and so is a key the table does not know.

## The search space

`space` takes factors as a [block](sweeps.md#blocks) does, each a parameter with `param` or the tick of an action with `action`.
A parameter the space leaves out keeps its `[set]` value or its default.
A parameter cannot be fixed in `[set]` and searched at once, and a factor can appear only once.

A range with no step spans every value from `min` to `max`, as in a sampled block.
A `u32` parameter and an action tick take the integers in it.
The search treats such a factor as ordered.
A change moves its value a short way up or down, and the search can close in on a good value.

A list of `values`, `levels = "all"` and a range with a `step` each give a set of values, and a value listed twice counts once.
The search ignores their order, and a change draws another value from the set at random.
Leave out the step of a range whose neighbouring values behave alike, and keep a list for values that do not, such as the options of a choice.

Henad holds a candidate as a **genome**, with one **gene** for each factor, a number `u` from 0 to 1.
An `f32` range with no step maps `u` onto its values in a straight line.
An integer range or a set of `m` values takes the value at position `floor(u * m)`.
A change to an ordered factor adds `mutation_scale * (r1 + r2 - 1)` to `u`, for two numbers `r1` and `r2` drawn uniformly from 0 to 1.
A step past either end folds back into the range.
`mutation_scale` is then the longest step, as a share of the range, and short steps come up more often than long ones.

The search draws its random numbers from a **search seed**, derived from the root seed and printed in the plan.
The draws use no logarithm or cosine, functions whose last bit can differ from one platform to another.
A spec gives the same candidates on every machine.

## Objectives

`objective` has three keys.

`column`
: The output that scores a candidate, a reducer column of `runs.csv` such as `Infected:max`.
  With the default reducers, every stat has its `:final`, `:min`, `:max` and `:mean` columns, and `[measure]` can add others.
  A column no reducer writes is refused before the first run, and the error lists the columns there are.

`goal`
: `maximize` or `minimize`.

`aggregate`
: `median` or `mean`, the rule that folds a candidate's replicates into one value.
  The median is the default, and one unusual run moves it less than it moves the mean.

A replicate that fails counts as the worst value there is: minus infinity when the goal is to maximize, and infinity when it is to minimize.
A replicate fails when its run fails, for example on a panic or a timeout, or when its value is empty or not finite.
With the mean, one failed replicate makes a candidate the worst of the search.
With the median, a candidate keeps a score while fewer than half its replicates fail.
A config that always crashes the model scores the worst value, and the search carries on past it.

Pattern Space Exploration takes no objective and refuses one.
Its two axes name the outputs it reads, as [its section](#pattern-space-exploration) shows.

## Replicates and seeds

Each evaluation runs its candidate `replicates` times.
The runs take their seeds from the seed scheme of `[seeds]`, as a sweep's do, with the candidate in place of the config.
Under common random numbers, the default, replicate `r` of every candidate starts from the same seed.
Two candidates then meet the same randomness, and a difference between them owes less to luck.

The genetic algorithm, and hill climbing when asked, can **re-evaluate** a candidate, running it again for more replicates.
A re-evaluation carries on from the candidate's last replicate index.
With four replicates, the first evaluation runs replicates 0 to 3 and a re-evaluation runs 4 to 7, on four new seeds.
The candidate's value then rests on all eight runs.
Under `scheme = "independent"`, a re-evaluation takes the seeds its candidate has at those replicate indices.

## Budgets and batches

`max_evaluations` is the budget of the search.
Each evaluation costs `replicates` runs, and a re-evaluation costs as much as a first evaluation.
The plan prints the runs the budget comes to, 1920 for the 480 evaluations of 4 replicates above.
The search ends once the budget is spent, and has no other stop.

A search runs in batches.
It asks its method for up to `batch_size` candidates, runs the batch on the lanes or tracks a sweep would use, several runs at once, and tells the method the results before it asks again.
A larger batch keeps more cores busy.
A smaller one lets the method react sooner.

The candidates of a search depend on its seed, its settings and the results it is told, and on nothing else.
Every table comes out the same byte for byte at any `--concurrent`, apart from the timing columns, and a second run of the spec gives the same search.
The exceptions are those of a sweep.
`gpu_boids` does not replay.
A run that times out ends wherever the clock caught it, and every candidate the search picks after it can change.

A search of a GPU model steps its runs on [GPU tracks](sweeps.md#gpu-tracks), as a sweep does, in the CLI and the desktop app.
The web app searches CPU models only.

## Random search

Random search, `algorithm = "random"`, draws every candidate uniformly from the space, as a `random` block draws its configs, and ranks the candidates by the objective.
It has no settings.

Random search is the baseline for the other three.
When another method does no better at the same budget, suspect an objective too noisy or too flat to climb.
It also suits a first look at a space of many factors, before a method that narrows in.

## Hill climbing

Hill climbing, `algorithm = "hill_climb"`, starts from the best of a batch of random candidates, the **incumbent**.
Each later batch holds `batch_size` **neighbours** of the incumbent.
A neighbour moves every ordered factor of the incumbent by up to `mutation_scale` of its range, and draws every other factor again.
The climb moves to the best neighbour when it beats the incumbent outright.
After `patience` batches in a row without a move, the climb starts over from a new batch of random candidates.

No two new candidates share a config.
A random candidate or a neighbour whose config repeats an earlier one is drawn again, up to 16 times.
When every draw repeats, the batch re-evaluates the earlier candidate in its place.
A re-evaluated random candidate competes for the start like the others, and a re-evaluated neighbour never moves the climb.
Over a few integers or options, a climb that has tried every nearby config stalls this way and starts over.

| Key | Default | Effect |
|---|---|---|
| `mutation_scale` | 0.1 | Longest step of a factor from the incumbent to a neighbour, as a share of its range |
| `patience` | 5 | Batches without a move before the climb starts over |
| `reevaluate` | `false` | Re-evaluate the incumbent in every batch, in place of one neighbour |

Hill climbing suits an output that changes smoothly with the parameters.
It closes in on the top of the nearest hill quickly, and the restarts give it a chance at other hills.
`best.csv` ranks the candidates of every climb, and the best can come from an earlier climb.

A noisy output misleads it.
A neighbour with lucky replicates can beat the incumbent and take its place.
With `reevaluate = true`, each batch runs the incumbent again with fresh replicates, and the value of a lucky incumbent falls back as its replicates pile up.
The incumbent takes one place in the batch, and a batch of one holds a neighbour alone.

## Genetic algorithm

The genetic algorithm, `algorithm = "genetic"`, evolves a **population** of candidates, one generation at a time.
Generation 0 is `population` random candidates.
Each later generation starts by re-evaluating the best members of the last.
It keeps the `elite_count` best members of the last generation unchanged, and fills the rest of the population with children.

A child needs a parent.
A **tournament** draws `tournament_size` members at random and takes the best of them as the parent.
With chance `crossover_rate`, a second tournament picks a second parent, and the child takes each factor from one parent or the other with even odds.
Otherwise the child starts as a copy of its one parent.
Each factor of the child then changes with chance `mutation_rate`, by a step of up to `mutation_scale`.

| Key | Default | Effect |
|---|---|---|
| `population` | 32 | Members of each generation, at most 65536 |
| `elite_count` | 2 | Best members carried unchanged into the next generation, fewer than `population` |
| `tournament_size` | 3 | Members drawn for each tournament, at most 65536 |
| `crossover_rate` | 0.9 | Chance that a child has a second parent |
| `mutation_rate` | 0.2 | Chance that each factor of a child changes |
| `mutation_scale` | 0.1 | Longest step of a changed factor, as a share of its range |
| `reevaluate_fraction` | 0.25 | Share of the population re-evaluated each generation, rounded up, best members first |

A larger tournament picks the best members as parents more often, and the population gathers around them sooner.
A generation takes as many batches as it needs, and the last of them ends with the generation.
In the example, each generation after the first holds 8 re-evaluations and 30 children, and runs as a batch of 32 and a batch of 6.

### Noise

The value of a candidate over a few replicates is partly luck, and the best of many candidates is often among the luckiest.
Each generation first re-evaluates the best `reevaluate_fraction` of the one before, with fresh replicates.
A member's **fitness** is its objective over every replicate it has, re-evaluations included.
A member that stays near the top keeps gathering runs, and its value settles.
The elites are chosen once the re-evaluations are in, and a leader that was lucky loses its place.
The parents of the generation's children are picked before then, from the values the members had when the last generation ended.

The example shows it happen.
Candidate 290 scored 125 on its first four runs, the best of the search at the time.
Its first re-evaluation scored 130 and its second 80, and over twelve runs its value fell to 105.
Candidate 409 led at 155 over eight runs, and fell to 130 over twelve.
Candidate 442 scored 107.5 on its first four runs and 165 on its next four, and won.
The best so far can drop from one batch to the next, and `batches.csv` shows it falling from 130 to 125 at batch 17.

A child can repeat the config of an earlier candidate, for example when it takes every factor from one parent and none mutates.
Under common random numbers, a copy would repeat the earlier candidate's first runs exactly, and its lucky first value with them.
The search mutates such a child again, up to 16 times, and no two new candidates share a config.
When every draw repeats, the generation re-evaluates the earlier candidate with fresh replicates, and that candidate joins the generation in the child's place.
Over a few integers or options, the search runs out of new configs this way and re-evaluates the ones it has.

## Pattern Space Exploration

Pattern Space Exploration (PSE), `algorithm = "pse"`, looks for variety.
It covers two outputs with a grid of cells, and tries to land candidates in as many cells as it can.
The filled cells map the pairs of the two outputs the model can produce, each with a config that produces it.

Here is a PSE of SIR over the peak and the tick of the peak.
Henad's repository keeps it at `crates/henad-explore/specs/sir_search_pse.toml`.

??? example "`sir_search_pse.toml`"

    ``` toml
    --8<-- "crates/henad-explore/specs/sir_search_pse.toml"
    ```

Save it as `sir_search_pse.toml` and run it:

``` bash
cargo run --release -p henad-cli -- --spec sir_search_pse.toml --out sir-pse
```

Above its `[search]` table, the spec reads like the genetic one, with 400 steps, two replicates and no action.

``` toml title="sir_search_pse.toml"
--8<-- "crates/henad-explore/specs/sir_search_pse.toml:search"
```

``` toml
--8<-- "crates/henad-explore/specs/sir_search_pse.toml:pse"
```

`x_axis` and `y_axis` each name a reducer column, and cut the range from `min` to `max` into `cells` cells of equal width.
Here the peak runs across 32 cells of 128 infected, and its tick up 20 cells of 2.5 ticks.
A value outside an axis lands in the edge cell nearer to it, and its evaluation is marked `outside`.
Many marked evaluations mean an axis is too narrow for the model.

An output's range is often unknown before the first run.
An axis written without `min` and `max`, as in `x_axis = { column = "Infected:max", cells = 32 }`, takes an **automatic range** from the initial samples.
The search holds its evaluations until the first `initial_samples` of them are told.
Each automatic axis then spans the smallest to the largest value those evaluations gave, widened by 5% of that span at each end.
A single value `v` gets a span of 1 around it, or of `|v|` when that is wider.
The range stays fixed for the rest of the search, and the evaluations held so far land in their cells.
Until then the archive is empty, each batch holds initial samples alone, and `evaluations.csv` gives each evaluation its values but no cell.
An axis takes both bounds or neither, and an automatic range needs at least one initial sample.
The manifest records the range of each axis as `axis_ranges`, and `archive.csv` gives the bounds of every cell.
`aggregate` folds the replicates into one value per axis, and leaves failed replicates out.
An evaluation with no value left on an axis lands in no cell.

The **archive** keeps every filled cell with its **exemplar**, the first candidate to land in it.
It also counts the cell's **hits**, every candidate that landed there.
The first `initial_samples` candidates are drawn at random, and a batch that reaches the last of them stops there.
Once every one of them is told, each later candidate comes from the archive.
The search draws two filled cells at random, keeps the one with fewer hits, and mutates its exemplar, moving every factor by up to `mutation_scale`.
The exemplars of rarely hit cells become parents more often, and the search spreads outward from the edges of the archive.
With `initial_samples = 0`, the first candidate is drawn at random, alone in its batch.
Until a candidate lands in a cell, the archive has nothing to breed from, and each later batch is drawn at random in full.

| Key | Default | Effect |
|---|---|---|
| `x_axis`, `y_axis` | | Each `{ column, min, max, cells }`, or `{ column, cells }` for an automatic range. Both needed |
| `initial_samples` | 64 | Candidates drawn at random before any is bred from the archive |
| `mutation_scale` | 0.1 | Longest step of a factor from its exemplar, as a share of its range |
| `aggregate` | `median` | Rule that folds the replicates into one value per axis |

The example fills 298 of its 640 cells.
The same 1200 evaluations drawn at random, with `initial_samples = 1200`, fill 167.
Most of the extra cells hold late peaks, after tick 25.
Few configs peak that late, and random draws seldom find them.
A cell that stays empty might lie beyond the model's reach, such as a peak before tick 5.
`filled_cells` in `batches.csv` shows the archive growing batch by batch, and a count that levels off suggests the search has reached most of the cells it can at that `mutation_scale`.

## The output directory

A search writes the files of a sweep and adds its own:

```text
sir-genetic/
├── batches.csv
├── best.csv
├── evaluations.csv
├── generations.csv
├── manifest.json
├── runs.csv
├── series.csv
└── summary.csv
```

`runs.csv`, `series.csv` and `summary.csv`
: As in a sweep, with the candidate's id as `config_id`.
  Run `i` of candidate `c` has the run id `c * replicates + i`, and its `rep` counts on from the candidate's first replicate index.
  A re-evaluation is a candidate of its own here, with its own row in `summary.csv`.

`evaluations.csv`
: One row per evaluation, with the candidate, its batch, the origin of its genome, its first replicate index, its config and its value.
  A re-evaluation names the candidate it repeats, and gives that candidate's value over every replicate it has.

`batches.csv`
: One row per batch, with the evaluations and runs so far, and the best candidate after the batch or the cells filled so far.

`generations.csv`
: One row per generation of a genetic algorithm, with the best, median and worst fitness of its members.

`best.csv`
: Every candidate of a random search, hill climb or genetic algorithm, best first, with its config, its value over every replicate and its replicate count.

`archive.csv`
: One row per filled cell of a PSE, in cell order, with the bounds of the cell, its hits, and the config and outputs of its exemplar.

`manifest.json`
: As in a sweep, with the mode `search` and a `search` section holding the budget, the search seed and the standing at the end.

The `origin` column of `evaluations.csv` names the source of a candidate's genome:

| Origin | Candidate |
|---|---|
| `random` | Drawn uniformly from the space |
| `restart` | Drawn uniformly as a hill climb starts over |
| `neighbor` | A neighbour of a hill climb's incumbent |
| `mutation` | A mutated copy of one parent |
| `crossover` | A child of two parents, then mutated |
| `reevaluation` | More replicates of the candidate in `reevaluated_id` |

`evaluations.csv`, `batches.csv` and `generations.csv` gain their rows as each batch ends, and a search you stop keeps them.
`best.csv` and `archive.csv` are written once the search ends.
The [CLI reference](../reference/cli.md#search-tables) lists every column.

### Reading the results

The app's [Results tab](app.md#search) plots a search with no code: the best value so far against the evaluations, the fitness of each generation, or the grid of a PSE as it fills.

In [pandas](https://pandas.pydata.org), `best.csv` is the place to start:

``` python
import pandas as pd

best = pd.read_csv("sir-genetic/best.csv")
print(best.head(5)[["candidate_id", "pooled_objective", "pooled_replicates", "evaluations", "action.second_wave"]])
```

```text
   candidate_id  pooled_objective  pooled_replicates  evaluations  action.second_wave
0           442             165.0                  8            2                 145
1           429             155.0                  8            2                 139
2           344             147.5                  8            2                 130
3           409             130.0                 12            3                 130
4           347             127.5                  8            2                 160
```

A candidate with more replicates has a value you can trust more.
The top five each rest on at least one re-evaluation, and candidate 409 on two.

`archive.csv` lists the regimes a PSE found.
Here are the latest peaks of the example:

``` python
archive = pd.read_csv("sir-pse/archive.csv")
print(archive.nlargest(3, "y")[["x", "y", "infection_rate", "recovery_rate"]])
```

```text
          x     y  infection_rate  recovery_rate
55    808.0  45.0        0.050579       0.088426
231  2815.5  42.5        0.050350       0.014846
253  3035.0  42.5        0.052105       0.011869
```

The latest peaks come from infection rates near the bottom of the range.
`archive.pivot(index="y_index", columns="x_index", values="hits")` turns the archive into the grid.

## Resuming a search

A search stopped part way resumes with `--resume`, as a sweep does:

``` bash
cargo run --release --locked -p henad-cli -- --spec sir_search_genetic.toml --out sir-genetic --resume
```

A method picks its candidates from its seed and from the results it is told, and a resume replays it from the start.
Each batch asks for the same candidates as before.
A run that `runs.csv` holds is read back in place of running again, and the runs after the last one written run as usual.
The finished directory is the same as that of a search that ran without a break, apart from the timing columns.

A resume tells the method every run as it was recorded, failed and timed-out runs included, and never runs one of them again.
A run that came out differently would change the candidates of every batch after it.
`--retry-failed` is refused for a search.

A resume also refuses a directory whose search differs in anything that sets its course: the model and its fixed values, the runs and their outputs, the seeds, the method and its settings, the budget, the batch size, the replicate count, the objective and the space.
The timeout and the execution settings can change.
To search beyond the budget, start a new search, for example over a space narrowed around the best candidates of the first.

Each batch depends on the batches before it, and a search cannot run as shards.
`--shard` is refused.
In the desktop app, open the directory in the [Results tab](app.md#opening-results) and press <span class="ui" markdown>:material-play: Resume search</span>.

## Searching in the app

The app's <span class="ui" markdown>:material-flask-outline: Sweep</span> tab runs a search in its **Search** mode.
Tick the parameters to search over, pick the **Method**, the **Objective** and the **Evaluations**, and press <span class="ui" markdown>:material-play: Start</span>.
Every setting on this page has a field there, and the [app tour](app.md#search-mode) names each one.
<span class="ui" markdown>:material-tray-arrow-down: Save spec</span> writes the search as a spec file for `henad-cli`, and <span class="ui" markdown>:material-tray-arrow-up: Load spec</span> reads one back.

While the search runs, the [Search view](app.md#search) of the <span class="ui" markdown>:material-chart-box-outline: Results</span> tab plots its course.
A click on a cell of a PSE grid selects the first run of the cell's exemplar, ready to open in the viewport.
In a browser, the app searches CPU models only, one run at a time.

*[CLI]: Command-line interface
*[CPU]: Central processing unit
*[GPU]: Graphics processing unit
*[CSV]: Comma-separated values
*[TOML]: Tom's Obvious Minimal Language
*[PSE]: Pattern Space Exploration
