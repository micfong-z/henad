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

The testing kit's `ThreadCount` check runs this comparison on every CPU model, at a size that splits a step into 14 jobs.
Both agent models carry a `results_do_not_depend_on_the_thread_count` test of their own as well, for a busier configuration than the kit's.
If your model draws random numbers during a step, consider one for it too.

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

## Network models

After `init`, a [network model](network-models.md) draws random numbers in three places, and each has a stream of its own.

The node pass is seeded like an agent pass.
The engine hands it a per-tick seed, and `run_pass` splits that seed per chunk through `chunk_seed`.
The global pass runs on one thread and draws from a single stream, in the order the model asks for numbers.
That stream carries over from one tick to the next.
An [action](parameters.md#actions) draws from a third stream, seeded apart from the other two.
A press advances neither the node pass's seed nor the global pass's stream.

Node positions are outside the contract.
The layout runs when a snapshot is published, under a time budget, and never inside `step`.
How far it has moved the nodes by a given tick depends on how many publishes there were and how many iterations each one fitted in.
Neither is a function of the tick.
The layout itself is deterministic for a fixed number of iterations, and `layout.rs` carries a thread-count test of its own.
Keep positions out of anything a tick decides.
Team Assembly reads one in its global pass to place newcomers next to their team's first incumbent, and that placement only changes the picture.

The order of neighbours inside a row is deterministic.
It follows from the sequence of edits made to the graph and from nothing else, and a repack keeps every row in order.
The order carries no meaning, though.
Removing an edge moves the last entry of each affected row into the gap, and flipping the direction rebuilds every row in edge-list order.
A kernel can walk a row in order, but it must not give an entry a meaning by where it sits, for example by reading the first entry as the oldest edge.

Each network model carries two tests.
If yours draws random numbers or writes anything in `prepare_view`, write both for it too.

`results_do_not_depend_on_the_thread_count` runs a busy configuration at 1 and at 7 threads, calls `prepare_view` after every tick, and compares the two runs bit for bit.
The population spans several chunks, and a parameter changes halfway through.
Virus on a Network runs with `keep_rewiring` on and switches `directed` on at the midpoint.
It compares the `state` and `timer` lanes, the edge list and the edge colours.
Team Assembly lowers `max_downtime` at the midpoint.
It compares the occupied slots, the spawn and team ticks, the positions, the edge list with its colours, the node colours and the bits of every stat.

`results_do_not_depend_on_the_publish_cadence` runs the same configuration twice, calling `prepare_view` after every tick in one run and never in the other, and asserts that both end in the same state.
The second run then calls `prepare_view` once, and the colours it paints have to match the first run's.
`prepare_view` can write to the graph and the lanes, and the test pins that nothing it writes feeds back into a tick.

Virus on a Network leaves positions out of both comparisons.
Team Assembly keeps them in, since its own tick places newcomers and neither test runs the layout.

## The testing kit

`henad_explore::testing`, behind henad-explore's `testing` feature, checks a model's entry against what its state does.
A model's tests take `henad-explore` as a dev-dependency with that feature on, and call `assert_set_conforms` over their model set.
It panics with every failure, and a set that passes prints which checks each model skipped.
`check_model_set` returns the same report without asserting anything.
The example models' test takes the report, to check each model's skipped checks as well.

```rust
--8<-- "crates/henad-models/src/tests/registry.rs:kit"
```

`headless_test_device` returns a device for the request, or `None` on a machine without one, and the GPU checks are then skipped.
With `HENAD_REQUIRE_GPU` set, a missing device fails the test instead, and so does every check a missing device would skip.
`TestDeviceRequest::baseline()` asks for the limits a browser offers.
A model that needs more takes `TestDeviceRequest::raised(models.gpu_needs())`, and a browser without those limits will refuse it.

| Check | Pins |
|---|---|
| `ModelId`, `ParamIds`, `StatLabels`, `ActionIds` | The id meets the grammar, and no parameter, stat or action id is declared twice |
| `Palette` | A declared palette has colours |
| `Metadata` | The backend, the structure, the topology hint and the device demand agree |
| `DefaultSetup` | The declared defaults pass `RunSetup::from_parts`, as the app's Build checks them |
| `DefaultsFit` | A GPU model's defaults fit a stock WebGPU device, checked without building |
| `ApplyModes` | A live parameter is accepted and a reload one is refused, exactly as declared |
| `Views` | The factory returns the declared backend, and its grid, point or edge views match the topology hint |
| `ParallelJobs` | Only a CPU model reports how many jobs a step splits into |
| `Actions` | Every declared action is accepted and an index past the last is refused, on a GPU model at its declared defaults |
| `StatCount` | `STATS.len()` matches what `stats` returns |
| `ThreadCount` | A CPU model's stats and exported state are the same at 1 and 7 threads |
| `SameSeed`, `SeedSensitivity` | Two runs on one seed agree, and two seeds differ in some stat or in the exported state |
| `SamplingCadence` | A run sampled every tick ends where a run sampled every seventh tick ends |
| `BaselineBuild` | A GPU model builds on the device exactly when the capacity check says it fits |
| `FullSubmission` | One submission of 64 steps reads back what 64 submissions of one step do, the OS watchdog trap of the [GPU backend](../developing/gpu-backend.md) |
| `SampledSlice` | A sampled slice of steps reads back what a snapshot does |

A check that builds the model sets the grid, population and world sizes small, so a model whose defaults hold ten million agents needs no settings.
`ThreadCount` sets the size itself, so that a step splits into 14 jobs.
At one job a kernel that shares state between chunks agrees with itself at any thread count.
The GPU checks build at the declared defaults, the size the app builds first.

A check the model cannot meet for an honest reason takes an exemption, recorded in the model's report.

```rust
--8<-- "crates/henad-explore/src/tests/kit.rs:exempt"
```

A model whose defaults need a device raised past the baseline fails `DefaultsFit`, and exempts it with its reason.
A browser refuses to build such a model at its defaults, and the exemption records that in the model's report.

A GPU model skips `ThreadCount`, since a pool width never reaches its kernels.
A model that declares `REPLAYS_EXACTLY = false` skips `SameSeed`, `SeedSensitivity` and `SamplingCadence`.
An exemption of a check that does not apply to the model fails that check, and the report of a set names every model the settings name and the set lacks.
`CheckSettings::set_text` sets a parameter in every check that builds the model, as `--set` reads it.
`check_model` returns the report instead of panicking, and its caller installs the panic hook first, through `henad_compute::fault::install_panic_hook`, or a kernel panic's failure names no `file:line`.

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

The two network models are compared with NetLogo the same way.
Their procedures are `virus_network_fixture.md` and `team_assembly_fixture.md` in `crates/henad-models/tests/fixtures/docs/`.
`scripts/compare_network.py` judges their runs as `compare_sir.py` judges SIR's.
On the Henad side, `crates/henad-models/tests/consistency_virus_network.rs` and `consistency_team_assembly.rs` hold the consistency tests, and they need no reference engine.
The Virus on a Network tests check every rate against the edge list instead of the model's own rows, and a row that drifted from the list fails them too.
The Team Assembly tests compare the lanes and the graph with a scan or a closed form instead of the model's own bookkeeping.

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
