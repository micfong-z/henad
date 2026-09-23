---
title: The command line
description: Every flag the headless henad-cli benchmark runner takes.
icon: material/console
---

# The command line

`henad-cli` is a headless benchmark runner that steps a model in a bare loop, with no rendering, no sim thread and no pacing.
A measurement therefore times `step()` and nothing else.

```text
henad-cli [OPTIONS] [MODEL]
```

`MODEL` is a model id, as printed by `--list`.

## Flags

| Flag | Default | Effect |
|---|---|---|
| `--list` | | Print the available model ids and exit |
| `--params` | | Print the model's parameters, with kinds, defaults and ranges, and exit |
| `--info` | | Print host and GPU details. Without a model, prints and exits. With a model, prints as a provenance header |
| `--json` | | Emit one JSON object per line instead of the human report, for a driver to parse |
| `--threads <N>` | 0 | Worker threads for CPU models. 0 leaves rayon's own choice, one per logical cpu |
| `--set <ID=VALUE>` | | Override one parameter. Repeatable |
| `--act <ID@TICK>` | | Run one of the model's actions at that tick. Repeatable |
| `--steps <N>` | 1000 | Steps to run and time per rep |
| `--reps <N>` | 1 | Independent timed runs, each on a freshly created state |
| `--warmup <N>` | 0 | Untimed steps before each rep, on that rep's own state, to reach a steady sim regime |
| `--global-warmup <N>` | 0 | Untimed steps once before the timed reps, to ramp GPU clocks and pay first-use compilation |
| `--seed <SEED>` | model default | RNG seed |
| `--export <PATH>` | | Write the final state after warmup and steps to this path, then exit |
| `--export-stats <PATH>` | | Write the per-tick stat series to this path as CSV, then exit |
| `--stats-every <N>` | 1 | Sample stats every N ticks when using `--export-stats` |
| `-h`, `--help` | | Print help |
| `-V`, `--version` | | Print version |

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

For a CPU model, `--export-stats` prepares the model's view before each sample, as the app does before it publishes a snapshot.
A stat computed during that preparation, such as Team Assembly's component stats, is then current in every row.
Each sample also pays for the preparation, and `--stats-every` spaces the samples out.

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

Always build with `--release`.
A debug build steps one to two orders of magnitude slower, and its timings mean nothing.
