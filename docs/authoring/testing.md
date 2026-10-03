---
title: Testing your model
description: The testing kit that checks every model of a set, the GPU checks and where they run, and the tests a model still needs of its own.
icon: material/test-tube
---

# Testing your model

Henad's **testing kit** checks a model's declarations against what its state actually does.
A project made from the template already runs it over every model in `models()`, and a model added there is checked by the next `cargo test`.

!!! info "Henad 0.3"

    This page describes Henad 0.3.

## The template's test

The test sits at the foot of the template's `src/lib.rs`:

``` rust title="src/lib.rs"
--8<-- "templates/model-project/src/lib.rs"
```

The kit is `henad::testing`, behind the facade's `testing` feature, and the template turns the feature on for its tests alone:

``` toml title="Cargo.toml"
[dev-dependencies]
henad = { version = "0.3", features = ["testing"] }
```

`assert_set_conforms` runs every check that applies to each model, and panics with every failure of the set.
A set that passes prints which checks each model skipped, and why.
`cargo test` hides that list for a passing test, and `cargo test -- --nocapture` shows it.

`check_model_set` returns the same report without asserting anything, and `check_model` checks one entry.
Henad's example models take the report, to check each model's skipped checks as well:

```rust
--8<-- "crates/henad-models/src/tests/registry.rs:kit"
```

`assert_skips_only_what_it_declares`, beside the test, asserts that a model skips only the checks its backend, its declared replay or a missing device rules out.

## A device for the GPU checks

`headless_test_device` acquires a GPU device without a window, or returns `None` on a machine without one.
The GPU checks are then skipped and listed, and the CPU checks still run.

`TestDeviceRequest::baseline()` asks for the limits a browser offers by default, and the template's test asks for it.
A model that binds more storage buffers than the baseline allows takes `TestDeviceRequest::raised(models.gpu_needs())`, with the needs of the set's GPU models, and a browser at the baseline refuses to build it.
Such a model also exempts `DefaultsFit`, which checks against the baseline on any device, as [Settings and exemptions](#settings-and-exemptions) shows.
`features` adds wgpu features to either request, and a test names them through `henad::gpu::wgpu`, with no `wgpu` dependency of its own:

```rust
use henad::gpu::wgpu;
use henad::testing::{TestDeviceRequest, headless_test_device};

let request = TestDeviceRequest::baseline().features(wgpu::Features::TIMESTAMP_QUERY);
let Some(device) = headless_test_device(&request) else { return };
```

A missing feature returns `None`, as a missing device does, and returns it even under `HENAD_REQUIRE_GPU`.
A test that asks for an optional feature checks for `None` itself.
An adapter below the baseline gives no device for either request.

Each test takes a device of its own, as the template's test does.
Clones of a device share one record of its errors, and a check takes an error it finds there as its own.
An error another test leaves on a shared device is then dropped, or reported as the failure of a check it has nothing to do with.

`HENAD_REQUIRE_GPU=1` turns a missing device into a failure, and fails every check a missing device would skip.
Set it wherever a GPU must be there.

## The checks

| Check | Pins |
|---|---|
| `ModelId`, `ParamIds`, `StatLabels`, `ActionIds` | The id meets the grammar, no parameter, stat or action id is declared twice, and the command line can name every parameter and action id |
| `Palette` | A declared palette has colours |
| `Metadata` | The backend, the structure, the topology hint and the device demand agree |
| `DefaultSetup` | The declared defaults pass `RunSetup::from_parts`, as the app's Build checks them |
| `DefaultsFit` | A GPU model's defaults fit a stock WebGPU device, checked without building |
| `ApplyModes` | A live parameter is accepted and a reload one is refused, exactly as declared |
| `Views` | The factory returns the declared backend, and its grid, point or edge views match the topology hint |
| `ParallelJobs` | Only a CPU model reports how many jobs a step splits into |
| `Actions` | Every declared action is accepted and an index past the last is refused, on a GPU model at its declared defaults |
| `StatCount` | `stats` returns a value for every entry of `STATS` |
| `ThreadCount` | A CPU model's stats and exported state are the same at 1 and 7 threads |
| `SameSeed`, `SeedSensitivity` | Two runs on one seed agree, and two seeds differ in some stat or in the exported state |
| `SamplingCadence` | A run sampled every tick ends where a run sampled every seventh tick ends |
| `BaselineBuild` | A GPU model builds on the device at its declared defaults, and the capacity check agrees that it fits |
| `FullSubmission` | One submission of 64 steps reads back what 64 submissions of one step do, the OS watchdog trap of the [GPU backend](../developing/gpu-backend.md). A model that does not replay exactly and declares stats reads back some stat that is not zero |
| `SampledSlice` | A sampled slice of steps reads back what a snapshot does |

A check that builds the model sets the grid, population and world sizes small, so a model whose defaults hold ten million agents needs no settings.
`ThreadCount` sets the size itself, for a step to split into 14 jobs.
At one job a kernel that shares state between chunks agrees with itself at any thread count.
The GPU checks build at the declared defaults, the size the app builds first.

A GPU model skips `ThreadCount`, since a pool width never reaches its kernels.
A model that declares `REPLAYS_EXACTLY = false` skips `SameSeed`, `SeedSensitivity` and `SamplingCadence`.

## Settings and exemptions

`CheckSettings` holds what the checks share.
`ticks` sets how many ticks a check steps, `thread_counts` the two pool widths `ThreadCount` compares, and `set_text` sets a parameter in every check that builds the model, as `--set` reads it.
An override of a parameter the model does not declare, or a value the parameter refuses, fails every check that builds the model, on a machine without a device as well.

A check the model cannot meet for an honest reason takes an exemption, recorded in the model's report:

```rust
--8<-- "crates/henad-explore/src/tests/kit.rs:exempt"
```

A model whose defaults need a device raised past the baseline fails `DefaultsFit`, and exempts it with its reason.
An exemption of a check that does not apply to the model fails that check.
The report of a set names every model the settings name and the set lacks, and a renamed model or parameter cannot leave a stale setting behind.

`check_model` returns the report instead of panicking, and its caller installs the panic hook first, through `henad::install_panic_hook`, or a kernel panic's failure names no `file:line`.

## Where the GPU checks run

The GPU checks run wherever `headless_test_device` finds a device: on your own machine, and on CI that installs a driver.
A runner on GitHub has no GPU.
The template's workflow installs lavapipe, a Vulkan driver that runs on the CPU, through `scripts/install-lavapipe.sh`, and sets `HENAD_REQUIRE_GPU=1`.

Lavapipe catches zero readbacks and a wrong step count, and has no watchdog.
`FullSubmission` guards against the OS watchdog, which fires only on real hardware.
There, too many passes in one command buffer leave every later readback reading zero, with no error.
Run `cargo test` on your own machine's GPU before you trust a GPU model.

## What stays hand-written

The kit checks that a model keeps its contract.
It has no oracle for what the model should compute, and the tests that check the rule itself stay yours:

- **A pattern drawn by hand.** The Game of Life tutorial checks a blinker against its known next states.
- **A closed form or an invariant.** [Checking the rule itself](determinism.md#checking-the-rule-itself) lists the kinds Henad's example models use.
- **A busier thread-count test.** `ThreadCount` runs a small configuration. A model that draws random numbers in several places can carry a [thread-count test](determinism.md#the-thread-count-test) of its own at a size where every path runs.
- **A comparison with another engine**, from a [written procedure](determinism.md#consistency-fixtures).
- **A GPU port against its CPU model.** A port that seeds itself through its CPU model's `init` starts on the same state at tick 0, and a test can compare the two backends while the model has no randomness of its own.

## Next

- [Determinism and testing](determinism.md) covers the contract the kit's determinism checks hold a model to.
- [Model sets](model-sets.md) covers the set the test reads.
