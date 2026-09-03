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

Always build with `--release`.
A debug build steps one to two orders of magnitude slower, and its timings mean nothing.
