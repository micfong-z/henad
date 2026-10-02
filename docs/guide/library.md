---
title: Using Henad from code
description: Building, stepping and sweeping models from a Rust program, and opening the app on a run.
icon: material/code-tags
---

# Using Henad from code

A Rust program reaches the app's and the command line's work through the `henad` crate.
It builds a model, steps it, reads its statistics, edits its parameters while it runs, sweeps it, reads the results back, and opens the app on any run.

!!! info "Henad 0.3"

    This page describes Henad 0.3.

## Adding Henad

A program depends on the `henad` crate alone, with the features it needs:

```toml title="Cargo.toml"
[dependencies]
henad = { version = "0.3", features = ["example-models", "app"] }
```

| Feature | Adds |
|---|---|
| `example-models` | The ten [example models](../reference/models.md), at `henad::models` |
| `app` | The app, at `henad::app` |
| `cli` | The command line, at `henad::cli`, on native targets |
| `testing` | The [testing kit](../authoring/testing.md), at `henad::testing` |

No feature is on by default, and none changes a result.
A program with models of its own starts from the [template](your-project.md) instead.
The template sets all of this up.

Copy the template's profile block into the program's `Cargo.toml` as well:

``` toml title="Cargo.toml"
--8<-- "templates/model-project/Cargo.toml:profile"
```

Cargo ignores the profiles of a dependency, and Henad's kernels compile at the opt-level of the crate that registers a model.
Without the block a debug build runs them at opt-level 0, and a model steps tens of times slower.
A release build runs them at 3, and Henad measures at 2.
Time anything with `--release`.

## The main types

- `ModelEntry` is one model, ready to build, with no device of its own.
- `ModelSet` is the entries a host offers, each id once.
- `RunSetup` holds parameter values by id, a seed and scheduled actions, each checked when set.
- `Simulation` is one built model that its caller steps.
- `StatSample` is one sample of a model's statistics, read by label.
- `SweepSpec`, `SweepOptions` and `SweepRecord` describe a sweep or a search and what it produced.
- `ResultSet` and `Replay` read a results folder and rebuild any run in it.

`henad::prelude` brings in all of them but two, with the other names a program like the one below uses.
`ModelEntry` sits at `henad::ModelEntry`, and `SweepRecord` at `henad::explore::SweepRecord`.

## A complete program

The program builds the example SIR model, runs it, reads its statistics, edits a parameter live, fires an action, runs a small sweep, reads the sweep back, rebuilds one run, and opens the app on it.

??? example "`complete.rs`"

    ```rust
    --8<-- "crates/henad/examples/complete.rs"
    ```

It prints:

```text
tick 100: Some(23836.0) susceptible
tick 100: Some(23814.0) susceptible, Some(12250.0) infected
tick 120: Some(145.0) susceptible, Some(16469.0) infected
tick 140: Some(75.0) susceptible, Some(5953.0) infected
...
tick 300: Some(75.0) susceptible, Some(0.0) infected
the epidemic ended by tick 300
12 of 12 runs ok
run 5 ends with Some(0.0) infected
```

and then opens the app on run 5 of the sweep.
The program is in Henad's repository as [`crates/henad/examples/complete.rs`](https://github.com/micfong-z/henad/blob/master/crates/henad/examples/complete.rs), and the rest of this page goes through it a part at a time.

## Building and stepping a model

```rust
--8<-- "crates/henad/examples/complete.rs:build"
```

`ModelSet::get` finds an entry by id, and `ModelEntry::setup` starts a `RunSetup` at the model's defaults.
`set` takes a value of the parameter's own type, and refuses a value of another type or out of bounds.
`set_text` reads the value as `--set` reads it, a choice by its option name included.
`with_seed` fixes the seed, and without it the model takes the default seed the command line takes without `--seed`.
`act_at` schedules an action at a tick, as `--act` does.

`build` fires the actions due at tick 0, and hands back the `Simulation`.
A GPU model builds on the device it is handed:

```rust
let gpu = henad::gpu::acquire_headless(models.gpu_needs())?;
let mut simulation = models.get("gpu_sir").ok_or("the example set holds GPU SIR")?.setup().build(Some(&gpu))?;
```

`acquire_headless` asks for a device without a window, with the limits the set's GPU models need.
Built with `None`, a GPU model returns a fault in place of a simulation.

`run_to` steps up to a tick and never past it, and `run_for` steps a number of ticks.
Each fires the scheduled actions after the step that reaches their tick, as a sweep's runs do.
`stats` samples the model as a sweep samples it, and a `StatSample` reads each stat by its label.
`views` hands a CPU model's grid, points and edges to a host that draws them, and `write_state` writes the state as `--export` does.

## Live edits and sampling

```rust
--8<-- "crates/henad/examples/complete.rs:live"
```

`set_param` edits a parameter between ticks, as a slider in the app does, and refuses a parameter that applies only on a rebuild.
`act` fires an action at once, on the current state.
A live edit takes effect from the next tick.

`run_sampled` steps to an end tick, and calls its closure with a sample at the start, at every multiple of the interval, and at the end.
The closure returns `ControlFlow::Break` to stop early, and `run_sampled` returns that value.
On a CPU model the closure runs inside the thread pool, on one of its workers, and needs `Send`.
The program collects its samples into a vector there and prints them afterwards.
A closure that holds something without `Send`, such as a window handle, steps through `run_to` and `stats` in a loop of its own instead.

### Thread pools

Henad runs a CPU model's kernels on rayon's thread pool.
Each call to `step`, `run_for`, `run_to` or `run_sampled` enters the pool once, and a call made from outside the pool pays for waking its workers.
`run_for` pays that once for the whole stretch, and a loop of `step()` pays it every tick.
On a small grid a tick costs less than the wake-up.

These were measured on a 14-core laptop, in release.
Each ratio is the time of a loop of `step()` over the time of Henad 0.2's own benchmark loop, which steps inside a scope of its own.
`run_for` came within 3% of that loop at every size, and a ratio within a few percent of 1 is run-to-run spread.

| Model and size | `step()` loop from `main` | `step()` loop inside `rayon::scope` |
|---|---|---|
| Game of Life, 64² | 12.2 | 1.01 |
| Game of Life, 256² | 2.05 | 0.98 |
| Game of Life, 1024² | 1.37 | 0.97 |
| Game of Life, 4096² | 1.07 | 1.02 |
| SIR, 64² | 8.71 | 1.01 |

Step with `run_for` or `run_to` where you can.
A loop that has to do something between two ticks goes inside `rayon::scope`, or `install` on a pool of your own, and then costs what `run_for` costs:

```rust
rayon::scope(|_| -> Result<(), henad::Fault> {
    for _ in 0..1000 {
        simulation.step()?;
        // Per-tick work of your own.
    }
    Ok(())
})?;
```

A GPU model takes no pool.
Each call waits for the device once, and `run_for` submits its steps in batches of up to 64.

## Faults and the panic hook

A model that panics, or a device that reports an error, ends the call with a `Fault`, and leaves the process running.
The fault names where the model panicked, as `file:line`, once the panic hook is installed:

```rust
henad::install_panic_hook();
```

No library call installs it, and a program calls it once in `main`.
The app and the command line install it for themselves.
The hook chains to the hook installed before it.
A program that installs a hook of its own afterwards keeps the previous one from `std::panic::take_hook` and calls it from the new one, or faults lose their location.

The hook keeps one list of recent panics for the whole process.
Two models that panic with the same message on two threads at once can swap their locations.

`GpuContext::new` takes over the device's error and loss handlers.
A host that shares its own wgpu device with Henad gets its errors reported as faults from then on.

No library call installs a logger either, and each `main` installs its own, as the template's does.

## Sweeps from code

```rust
--8<-- "crates/henad/examples/complete.rs:sweep"
```

`LoadedSpec::parse` reads the same TOML as a [spec file](sweeps.md#spec-files), and `LoadedSpec::read` reads one from a path, with its design table.
A `SweepSpec` can also be built in code, from the types in `henad::explore::spec`.

`SweepOptions::new` takes the `Provenance` the manifest records: the program's own build and its command line.
`apply_execution` copies the spec's `[execution]` table into the options.
Set the program's own settings after it, as `--concurrent` overrides the table on the command line.
`concurrency`, `memory_budget` and `gpu_memory` decide how the runs spread over the machine, `shard` and `resume` act as `--shard` and `--resume` do, and none of them changes a result.

`run_spec` runs the sweep, or the search for a spec with a `[search]` table, and blocks until it ends.
`SweepOutput::Directory` writes the four files of the [output directory](sweeps.md#the-output-directory), and `SweepOutput::Memory` hands them back in the `SweepRecord` in place of a folder.
The `Progress` argument hears of each planned sweep, finished run and warning, and `NoProgress` ignores them all.
`plan_spec` plans the sweep without running it, as `--dry-run` does.
For a sweep that runs while the program does something else, `SweepRun::start` runs it on a thread of its own, and reports through a channel of `SweepEvent`s.

A GPU model sweeps on the device it is handed, or on a headless device of its own when it is handed `None`.

## Reading results back

```rust
--8<-- "crates/henad/examples/complete.rs:replay"
```

`ResultSet::open_dir` reads a results folder: the manifest, every complete row of `runs.csv`, and the series of each run while the byte budget lasts.
`ResultSet::replay` checks a run's row against the plan the folder records and returns its `Replay`, the model, values, seed and schedule that rebuild it.
`RunSetup::from_replay` turns it back into a setup.

A run's trajectory depends on its model, its values, its seed and its schedule, and on nothing else.
A rebuild on the same build of Henad and of the model steps through the same states, and its stats at the run's last tick equal the run's last row in `series.csv`.
A pause, the thread count or the number of runs stepping beside it never changes a run.

Three things stand between a run and its rebuild:

- **A model that does not replay exactly.**
  A GPU model whose passes leave the order of their writes to the device declares `REPLAYS_EXACTLY = false`.
  GPU Boids is the one example model that does.
  Its rebuild starts the same and drifts.
- **A changed build.**
  The manifest records the build of Henad and of the model's crate for every session that wrote runs.
  A rebuild from a changed build steps the changed code, and the app warns before it opens such a run.
- **A sample taken on another schedule.**
  A rebuild sampled from tick 0, as `run_sampled` and `--export-stats` sample, holds other ticks than a series that started after a warm-up.
  The ticks both hold agree.

## Opening the app

```rust
--8<-- "crates/henad/examples/complete.rs:app"
```

`AppOptions::new` takes the set the app offers, the product name and the program's build.
The product name is the window's title, and names the folder where the app keeps its settings.
`opening` decides what the app opens on:

| Opening | Opens |
|---|---|
| `AppOpening::Run { replay, open_at }` | A recorded run, built and stepped to `open_at` |
| `AppOpening::Setup { setup, open_at }` | A `RunSetup` the program built, with its values, seed and schedule |
| `AppOpening::Results(folder)` | A results folder in the Results tab, on native targets |

`OpenAt::Start` opens the run paused at tick 0, and `OpenAt::Tick(t)` steps it to tick `t` first.
The app checks the opening against the set before it opens a window, and `run_native` returns an error for a model the set lacks.
It blocks until the window closes.

In a browser, `start_web` takes a closure that builds the options, and calls it once the thread pool has started.
Nothing that runs before `start_web` may touch rayon, or the pool starts with one thread.
The template's `src/main.rs` shows both entry points.

## Provenance

Every results folder records three builds: Henad's, the host's, and that of the crate that registered each model.
A build is a crate's name and version, the commit it was built from, whether its sources differed from that commit, and a hash of its sources.
A resume, a merge or a replay compares the recorded builds with its own, and warns when Henad's build or the model's differs.

`henad::build_info!()` returns the build of the crate it expands in.
A crate that holds models builds its set with `ModelSet::new(henad::build_info!())`, and the set records that build on each model inserted into it.
That crate also calls `henad_build::stamp_commit()` from its `build.rs`, with `henad-build` among its build dependencies:

```rust title="build.rs"
fn main() {
    henad_build::stamp_commit();
}
```

Without the stamp the build reads as unknown, and an uncommitted edit to a model goes unrecorded.
Two unknown builds never count as the same, and every resume warns whether or not the model changed.
A program that registers no models of its own, as the one above, needs no `build.rs`.

The stamp hashes the files under the crate's `src` and its manifest.
A file a model reads at compile time, through `include_bytes!` or `include_str!`, belongs under `src`, where the hash sees it.

henad-core has no hash of its own sources.
An edited copy of henad-core, pulled in through `[patch]`, reads as the release it patches, and a resume does not warn about it.

## Next

- [Your own project](your-project.md) sets up a project with models of its own, its app and its command line.
- [Parameter sweeps](sweeps.md) covers every setting of a spec.
- The [API reference](../reference/api.md) links each crate's documentation on docs.rs.
