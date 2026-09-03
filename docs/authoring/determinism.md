---
title: Determinism and testing
description: The determinism contract every Henad model has to hold, and how to test it.
icon: material/check-decagram-outline
---

# Determinism and testing

Henad runs every kernel in parallel and gives no guarantee about which chunk lands on which core.
The answer still has to come out the same every time, and this page covers the rules that make sure it does.

A chunk's RNG comes from `chunk_seed(base, tick, chunk_index)` and from nothing a worker mutates, which keeps a result independent of how rayon schedules the chunks.
The same has to hold on the web, where the pool width is whatever `navigator.hardwareConcurrency` reported.

Reductions obey the same rule.
`reduce_chunks` folds its partials in chunk order rather than completion order, a `Tally` merges in chunk order, and the [scatter](fields.md#the-scatter) totals in fixed point before touching `f32`.
Float addition is not associative, so if any of these folded in arrival order instead, a stat would depend on the machine that ran it.

!!! warning "Every draw needs its own `next_bits`"

    Feeding one word to two draws correlates them, and neither the compiler nor a test will tell you it happened.
    The split into an advancing call plus pure draws exists so that the draws can be parity-tested against WGSL, and sharing one word between draws defeats the point of that split.

## The thread-count test

Both agent models carry a `results_do_not_depend_on_the_thread_count` test.
If your model draws random numbers during a step, write one for it too.

```rust
#[test]
fn results_do_not_depend_on_the_thread_count() {
    fn run(threads: usize) -> Vec<u32> {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build()
            .expect("rayon pool");
        pool.install(|| {
            let params = vec![ParamValue::U32(20_000), ParamValue::F32(800.0), ParamValue::F32(800.0)];
            let mut state = State::from_params(&params);
            for _ in 0..30 {
                state.step();
            }
            let lanes = state.lanes();
            lanes
                .pos_x
                .iter()
                .zip(&lanes.pos_y)
                .flat_map(|(x, y)| [x.to_bits(), y.to_bits()])
                .collect()
        })
    }
    assert_eq!(run(1), run(7), "boid positions depend on the thread count");
}
```

Three details of the test matter.

- Compare **bits** rather than floats, because `to_bits` catches a last-bit difference that `assert_eq!` on `f32` would report but an `approx` comparison would hide.
- Use a **population that spans several chunks**, preferably one that is not a multiple of `CHUNK`, so that the ragged final chunk is covered too.
- Use **thread counts that are not multiples of each other**, since running 1 against 7 splits the work completely differently.

To run a single test by name:

```bash
cargo test -p henad-models results_do_not_depend_on_the_thread_count
```

## Tests the registry brings

Registering a model brings a set of tests with it for free, and they cover GPU entries too when a device is available.

| Test | Pins |
|---|---|
| `declared_apply_mode_matches_what_the_state_accepts` | A live parameter is accepted and a reload one is rejected, exactly as declared |
| `declared_topology_matches_the_views_the_state_returns` | A model that says it draws a grid actually publishes one |
| `every_declared_stat_series_gets_a_value` | `STATS.len()` matches what `stats` returns |
| `every_gpu_model_builds_on_a_baseline_device` | Every GPU model builds on a stock WebGPU device |
| `every_gpu_entry_reports_its_capacity` | The capacity check agrees with what actually builds |
| `a_kernel_that_panics_mid_step_keeps_its_location` | A bug in a kernel reaches the UI with its `file:line` |

A registered model whose declarations are accurate therefore starts with a baseline of coverage, before you write any tests of your own.
See [registering a model](registering.md).

## Checking the rule itself

Determinism aside, the model's rule still needs an oracle of its own.
The repository leans on three kinds, in descending order of strength.

**A bit-identical reference.** This is only available when nothing in the model is stochastic.
`gpu_game_of_life` is checked against its CPU counterpart this way, and Game of Life is checked against fixtures recorded from NetLogo.

**A closed form.** SIR's infection rate is checked against `1 - (1 - beta)^n` over a large grid, with a tolerance band derived from the binomial spread rather than picked by hand, and its recovery rate is checked the same way.

**An invariant.** This is the weakest kind of oracle, but it is always available.
Examples in the repository include population being conserved, transitions only going forwards, deliveries never decreasing, ants staying on the lattice, inside the world and off obstacles, and a cell with no ant on it decaying by exactly the evaporation rate.

The consistency tests live in `crates/henad-models/tests/` and are named `consistency_<model>.rs`.

## Consistency fixtures

A fixture recording another engine's output has to come from a **written procedure**, and a generation script does not qualify.
The procedure goes in `crates/henad-models/tests/fixtures/docs/` for a human to run.

A driver script would presume the reference engine is installed, which no future collaborator can be expected to have.
The committed fixture together with its procedure is the reproducibility record.

Where the reference engine is code rather than a GUI, a small committed program *is* the procedure, and that is fine.

!!! danger "Never generate a fixture from Henad"

    A fixture produced by the engine under test only proves that the engine agrees with itself, so a test against it passes forever and means nothing.

For a stochastic model the two engines draw from different generators, and a fixture cannot then be compared point by point.
`scripts/compare_sir.py` instead compares the distribution of summary statistics over many replicates, against margins derived from Henad's own measured run-to-run spread.

## Before calling it green

```bash
./check.sh
```

This script runs the CI-equivalent check set.
GPU tests skip silently on a machine with no adapter, so set the environment variable that turns the skip into a failure:

```bash
HENAD_REQUIRE_GPU=1 cargo test --workspace --all-targets
```

## Next

- [Writing fast models](performance.md) covers the performance constraints that come with the same parallelism.
- [Contributing](../developing/contributing.md) explains what a change has to pass.
