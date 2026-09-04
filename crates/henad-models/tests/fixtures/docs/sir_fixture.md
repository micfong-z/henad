# Generating the SIR consistency fixture

> [!note]
> This is a distributional test of the SIR model. For a deterministic self-consistency check, see `crates/henad-models/tests/consistency_sir.rs`. Compare to `scripts/compare_sir.py` for further details.

Unlike Game of Life and boids there is no fixture and no assertion in `cargo test`. SIR is stochastic, both engines draw from different generators, and a single run carries no information. The output is a report over many replicates.

## Reference Model

NetLogo's library has no grid-based SIR. The reference model is in `crates/henad-models/tests/fixtures/sir/sir_netlogo.nlogox`. This model is created under the rules as stated in [§Rules](#rules).

## Rules

Each tick, simultaneously for every cell:

- **S**, with `k` infected Moore neighbours: becomes **I** with probability `1 - (1 - beta)^k`
- **I**: becomes **R** with probability `gamma`
- **R**: stays **R**

The world is toroidal, the neighbourhood is Moore (8 cells), and cells never move backwards through S → I → R.

### Simultaneous updates

`ask patches` is sequential. Computing and applying a cell's next state in one pass would let a cell see neighbours that had already changed this tick. Hence we use two `ask patches` blocks below, similar to the split Wilensky's Life uses.

## World setup

In _Settings_:

- **Location of origin**: Corner, Bottom Left
- `max-pxcor` **255**, `max-pycor` **255** — the dialog should read `Torus: 256 x 256`
- both **wrap** boxes checked

## Parameters

`beta` 0.08, `gamma` 0.3, initial infected 1%.

> [!note]
> **Model defaults are not used.** At Henad's defaults (`beta` 0.3, `gamma` 0.05), every cell is eventually infected, so the final recovered fraction is exactly 1.0000 on every run of every engine. At `8 * beta / gamma` around 2 the epidemic infects roughly a third of the grid and leaves genuine run-to-run spread, which is more useful for this comparison.

## Procedure

Open the model at `crates/henad-models/tests/fixtures/sir/sir_netlogo.nlogox` in NetLogo, and run the following in the Command Center.

```netlogo
run-replicate "sir_netlogo_01.csv" 1 300
```

The matching Henad runs:

```bash
cargo run --release -p henad-cli -- sir \
  --set grid_width=256 --set grid_height=256 \
  --set infection_rate=0.08 --set recovery_rate=0.3 --set initial_infected_pct=0.01 \
  --steps 300 --seed 1 --export-stats sir_henad_01.csv
```

Both write the same four columns, so one comparison script reads either.

## Margins

Three summary statistics per run: **peak infected fraction**, **tick of that peak**, and **final recovered fraction**.

We compute the difference in means, its 95% confidence interval, and require that the whole interval lies inside the margin.

Margins come from Henad's own measured run-to-run spread. At `beta` 0.08, `gamma` 0.3, 1% initial:

| grid    | seeds | peak I frac             | tick of peak | final R frac            |
| ------- | ----- | ----------------------- | ------------ | ----------------------- |
| 128x128 | 30    | 0.0313 ± 0.0038 (12.0%) | 12.9 ± 2.66  | 0.3255 ± 0.0362 (11.1%) |
| 256x256 | 30    | 0.0303 ± 0.0017 (5.7%)  | 10.8 ± 1.36  | 0.3238 ± 0.0220 (6.8%)  |
| 512x512 | 15    | 0.0301 ± 0.0007 (2.3%)  | 10.8 ± 0.94  | 0.3251 ± 0.0085 (2.6%)  |

We choose 256x256 as a compromise between runtime and margin width.

**Margins for 256x256, 50 seeds per engine**

| statistic                | margin     | 95% CI half-width | headroom |
| ------------------------ | ---------- | ----------------- | -------- |
| peak infected fraction   | ±0.004     | 0.00067           | 6.0x     |
| tick of peak             | ±1.5 ticks | 0.53              | 2.8x     |
| final recovered fraction | ±0.03      | 0.0086            | 3.5x     |

CI half-width is $\frac{1.96 \sigma \cdot \sqrt{2}}{\sqrt{50}}$, where $\sqrt{2}$ arises because the difference of two independent means has twice the variance of one.

Each margin is several times the CI half-width, so two correct engines should pass comfortably.

## Running the comparison

`scripts/compare_sir.py`. It reads both directories of CSVs, and generates the Henad-side results automatically. The reference side is always produced manually from the procedure above.

```bash
cargo build --release -p henad-cli
uv run --project scripts scripts/compare_sir.py --reference path/to/reference_csvs --generate 50
```

The script reports `EQUIVALENT`, `INCONCLUSIVE` or `DIFFERENT` per statistic. `INCONCLUSIVE` indicates that the interval is wider than the margin, so the replicate count is too low to decide, and more runs are needed.

A two-sample KS test is also printed as a diagnostic, since it could catch shape differences apart from the mean. However, it is a difference test, so it can reject a correct pair at its own alpha, and with enough replicates it rejects differences far too small to be practically relevant.

## Result

50 replicates per engine, on a 256x256 grid, with `beta` 0.08, `gamma` 0.3, 1% initial, 300 ticks:

| statistic                | Henad             | NetLogo 7.0.4     | difference (95% CI) | margin | verdict    |
| ------------------------ | ----------------- | ----------------- | ------------------- | ------ | ---------- |
| peak infected fraction   | 0.03031 ± 0.00171 | 0.03006 ± 0.00194 | 0.00025 ± 0.00073   | ±0.004 | EQUIVALENT |
| tick of peak             | 10.88 ± 1.44      | 11.12 ± 1.52      | -0.24 ± 0.59        | ±1.5   | EQUIVALENT |
| final recovered fraction | 0.32485 ± 0.02009 | 0.32319 ± 0.01634 | 0.00166 ± 0.00727   | ±0.03  | EQUIVALENT |

KS diagnostics were unremarkable (`D` 0.12 to 0.18, `p` 0.40 to 0.87).

## Other engines

The same comparison, against the ports written for the cross-engine benchmarks. Each is run by
`scripts/validate_ports.py`, which drives that engine's harness under `benchmarks/<engine>/` and
then this script.

Mesa 3.5.1, same configuration and replicate count:

| statistic                | Henad             | Mesa 3.5.1        | difference (95% CI) | margin | verdict    |
| ------------------------ | ----------------- | ----------------- | ------------------- | ------ | ---------- |
| peak infected fraction   | 0.02990 ± 0.00250 | 0.02969 ± 0.00181 | 0.00021 ± 0.00087   | ±0.004 | EQUIVALENT |
| tick of peak             | 11.04 ± 1.78      | 10.88 ± 1.64      | 0.16 ± 0.68         | ±1.5   | EQUIVALENT |
| final recovered fraction | 0.32560 ± 0.01852 | 0.32196 ± 0.01466 | 0.00364 ± 0.00663   | ±0.03  | EQUIVALENT |

KS diagnostics again unremarkable (`D` 0.12 to 0.18, `p` 0.40 to 0.55).

---

This document is assisted with Claude Opus 5, with heavy human edits after generation.
