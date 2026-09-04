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

Measure one thread against every core, which is what the cross-engine comparison reports:

```bash
cargo run --release -p henad-cli -- boids --threads 1 --steps 100 --reps 5 --json
```

## Machine-readable output

`--json` replaces the report with one JSON object per line on stdout, leaving progress on stderr.
An `info` line comes first, then one `rep` line per timed rep as it finishes, then a `summary`.
A run killed part way still reports the reps it managed.

```json
{"kind":"info","engine":"henad","engine_version":"0.1.0","model":"boids","variant":"cpu","threads":1}
{"kind":"rep","rep":0,"seed":42,"steps":100,"elapsed_s":1.234,"population":50000,"heap_bytes":2050020}
```

This is the same shape every other engine in the [benchmarks](../benchmarks.md) speaks, so one
driver reads them all.
`--info --json` prints host and adapter details in the same stream.

Always build with `--release`.
A debug build steps one to two orders of magnitude slower, and its timings mean nothing.
