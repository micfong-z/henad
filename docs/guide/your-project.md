---
title: Your own project
description: Starting a project of your own from the template, with its app, command line, web build, tests and CI.
icon: material/folder-plus-outline
---

# Your own project

A project of your own holds your models, and runs them in an app, a command line and a web build of its own, on the Henad crates published to crates.io.
It starts as a copy of the template in Henad's repository, `templates/model-project`.

!!! info "Henad 0.3"

    This page describes Henad 0.3.

## Getting the template

Fetch the template from the release it belongs to.
Its `Cargo.toml` then requires that release's crates.

--8<-- "templates/model-project/README.md:fetch"

The project starts with two models of the voting rule, `vote` on the CPU and `gpu_vote`, its port to the GPU.
The [first-model tutorials](first-model/game-of-life.md) add models beside them, and you can delete either once you have your own.

## Renaming it

The package can be renamed alone, and every command keeps working.
Renaming the package renames the library too, and the `my_model::` paths in the two binaries follow it.
A full rebrand renames these:

--8<-- "templates/model-project/README.md:rename"

The product name is the app's window title, and names the folder where the native app keeps its settings, with each space turned into a dash.
"My Model" keeps its settings in a folder named `My-Model`.

## What is in it

```text
my-model/
├── Cargo.toml
├── README.md
├── build.rs                   # shader bindings and the commit stamp
├── rust-toolchain.toml        # the stable toolchain Henad pins, with the wasm32 target
├── rustfmt.toml               # the 120 columns Henad formats at
├── .gitignore
├── .cargo/config.toml         # the wasm32 flags of the threaded web build
├── index.html                 # the web page, for Trunk
├── Trunk.toml                 # file names, the two headers and the port of the web build
├── assets/icon-256.png
├── specs/vote.toml            # a small sweep over the initial density
├── scripts/
│   ├── build_web.sh           # the web build, on the dated nightly
│   ├── ci.sh                  # the stages CI runs
│   ├── install-lavapipe.sh    # a GPU driver that runs on the CPU, for CI
│   ├── trunk-version          # the Trunk release, one line
│   └── web-toolchain          # the dated nightly, one line
├── src/
│   ├── lib.rs                 # models(), and the test of every model in it
│   ├── main.rs                # the app, natively and on the web
│   ├── bin/my-model-cli.rs    # the command line
│   ├── vote.rs                # a CPU model
│   └── gpu_vote/              # its GPU port: mod.rs and three shaders
└── .github/
    ├── workflows/ci.yml       # runs scripts/ci.sh
    └── dependabot.yml         # keeps the dependencies current
```

`src/lib.rs` is the library, and its `models()` is the one place a model is registered.
The app, the command line and the test all read their models from it.

```rust title="src/lib.rs"
--8<-- "templates/model-project/src/lib.rs"
```

The `.gitignore` lists `/target` and `/dist`, the two folders a build writes, and the files Finder and editors leave beside sources.
It never lists `Cargo.lock`.
A project that keeps sweep folders inside its tree adds `/runs` to it.

## Running it

--8<-- "templates/model-project/README.md:run"

Run a model with `--release`.
The dev profile builds the crate holding your kernels at opt-level 1, with overflow checks and debug assertions on, and a model steps several times slower there.
A toy kernel at opt-level 1 ran about 4.7 times slower than at 2, in single runs.
A plain `cargo run --bin my-model-cli -- vote --params` is enough for the commands that step nothing.

The command line has every mode of Henad's own, sweeps, searches, shards and exit codes included.
[The CLI reference](../reference/cli.md) lists them under the name `henad-cli`, and in your project the binary is `my-model-cli`.

## The lock file

The template ships no `Cargo.lock`.
The first build writes one, and you commit it.
A sweep split into shards, or resumed later, then runs the same dependencies on every machine and every day it runs, as long as each build passes `--locked`:

```bash
cargo run --release --locked --bin my-model-cli -- --spec specs/vote.toml --shard 0/4 --out runs/vote-0
```

A resume or a merge warns when the recorded builds differ, and [resuming a sweep](sweeps.md#resuming-a-sweep) says what the warning compares.

## Build profiles

Cargo ignores the profiles of a dependency, and the published crates carry none.
Henad's kernels are generic, and they compile in the crate that registers a model, at that crate's opt-level.
The template's `Cargo.toml` sets the profiles Henad measures at:

``` toml title="Cargo.toml"
--8<-- "templates/model-project/Cargo.toml:profile"
```

Without this block a debug build runs your kernels at opt-level 0, and a release build at 3.
Every Henad number is measured at 2.
Keep it when you copy the dependencies into another project.

## The build script

```rust title="build.rs"
--8<-- "templates/model-project/build.rs"
```

`ShaderBuild::discover("src")` generates the Rust bindings of every shader under `src`.
A GPU model added later joins them on the next build, and no build file changes.
[Shaders and bindings](../authoring/shaders.md) covers the bindings.

`stamp_commit` records the commit the crate was built from, whether its sources differed from it, and a hash of the sources.
Every results folder records that build beside each model, and a resume, a merge or a replay compares it with the build that runs it.
Without the stamp the build reads as unknown, and an uncommitted edit to a model goes unrecorded.
Two unknown builds never count as the same, and every resume warns whether or not the model changed.
A crate without shaders drops the `ShaderBuild` line and `henad::include_shaders!()`, and keeps `build.rs` for the stamp.

The stamp sees what sits under `src` and the manifest.
A file a model reads at compile time, through `include_bytes!` or `include_str!`, belongs under `src`, or a change to it goes unrecorded.
The app's icon sits in `assets/`, since it changes no result.

A commit reruns the build script, even one that changes no source.
The crate holding your kernels then recompiles, and both binaries relink.
Henad's own crates stay built.

## Updating Henad

--8<-- "templates/model-project/README.md:update"

Henad's crates require each other at the same version, and `cargo update -p henad` moves them all.
`henad-build` alone would move henad-core and leave the engine behind it.
The template's `.github/dependabot.yml` groups `henad` and `henad-build`, and a pull request never moves one without the other.

A new minor release can change the API, and the [CHANGELOG](https://github.com/micfong-z/henad/blob/master/CHANGELOG.md) gives each change with what to do about it.
[Releasing](../developing/releasing.md#stability) describes what a release may change.

## The web build

```bash
scripts/build_web.sh serve --release      # serves on http://127.0.0.1:8081
scripts/build_web.sh build --release      # writes dist/
```

The web build runs a thread pool in the browser, and needs a nightly toolchain with a standard library rebuilt for it.
The script reads the dated nightly from `scripts/web-toolchain`, and prints the command that installs it when it is missing:

```bash
rustup toolchain install "$(cat scripts/web-toolchain)" --profile minimal \
  --component rust-src,clippy --target wasm32-unknown-unknown
```

Trunk is the other tool it needs, at the release `scripts/trunk-version` names:

```bash
cargo install --locked trunk --version "$(cat scripts/trunk-version)"
```

The wasm32 flags live in `.cargo/config.toml`, and a flag of your own for the web goes into its array.
An exported `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` would replace that array without notice, and the build would fail on an error that names neither.
The script refuses to run while either is set, even to an empty value.

A host serving `dist/` sends two headers, or the app runs on one thread:

```text
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

GitHub Pages cannot send them, and a site there needs a service worker such as [coi-serviceworker](https://github.com/gzuidhof/coi-serviceworker) to add them.
A browser keeps one web app's saved settings per origin.
Serve each app from an origin of its own.

## Checks and CI

`scripts/ci.sh` runs four stages, or all four without an argument:

| Stage | Runs |
|---|---|
| `lint` | Clippy over the package, warnings denied |
| `lint-web` | Clippy for the threaded web build, on the nightly |
| `test` | The tests, the check of every model in `models()` among them |
| `web` | The release web build, then a check that `dist/` holds the worker glue |

Each stage passes `--locked` once a `Cargo.lock` exists.
`.github/workflows/ci.yml` runs them on every push to `main`, on every pull request, and when started by hand.

The test at the foot of `src/lib.rs` runs Henad's [testing kit](../authoring/testing.md) over every model in `models()`, and a model added there is checked by the next `cargo test`.
A runner on GitHub has no GPU, and the workflow installs lavapipe, a Vulkan driver that runs on the CPU, through `scripts/install-lavapipe.sh`.
It sets `HENAD_REQUIRE_GPU=1`, and a GPU check that would be skipped fails instead.
Lavapipe has no watchdog.
Run `cargo test` on your own machine's GPU as well.

`cargo deny` reports `fxhash` as unmaintained, under RUSTSEC-2025-0057.
It reaches your project as a build dependency of henad-build's shader generator, and never runs in a binary.
Henad's own `deny.toml` ignores it with that reason.

## Publishing a model library

The template is already the shape of a library: the app and the command line sit behind the package's own features `app` and `cli`, both on by default.
To publish your models as a crate:

1. Remove `publish = false` from `Cargo.toml`.
2. Set `license` and `description`, and an `exclude` list for what only the project needs: `index.html`, `scripts/`, `.cargo/` and `specs/`.
3. Keep the `app` and `cli` features, so that a consumer can turn both off.
4. Keep `build.rs` and its `stamp_commit()` line.

A consumer depends on the library with its features off, and adds its models to a set of its own:

```toml
[dependencies]
my-model = { version = "0.1", default-features = false }
```

```rust
let mut models = henad::ModelSet::new(henad::build_info!());
models.extend(my_model::models()?)?;
```

Each entry keeps your crate's name and version as its source, and a consumer's results folder records them.
Without `default-features = false`, every consumer would compile the app and the command line, since Cargo merges the features every dependent asks for.
A model library is compatible with one Henad 0.x at a time, and moves to the next with it.

## Next

- [Writing a CPU grid model](first-model/game-of-life.md) adds a model to the project, step by step.
- [Model sets](../authoring/model-sets.md) covers `models()` and the entries in it.
- [Testing your model](../authoring/testing.md) covers the checks the template's test runs.
