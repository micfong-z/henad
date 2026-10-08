---
title: Your own project
description: Starting your own project from the template, with its app, command line, web build, tests and CI.
icon: material/folder-plus-outline
---

# Your own project

Your own project holds your models, and runs them in its own app, command line and web build, on the Henad crates published to crates.io.
It starts as a copy of the template in Henad's repository, `templates/model-project`.

!!! info "Henad 0.3"

    This page describes Henad 0.3.

## Getting the template

Fetch the template from the release it belongs to.
Its `Cargo.toml` then requires that release's crates.

--8<-- "templates/model-project/README.md:fetch"

On Windows, the commands on this page run in Git Bash or WSL.

The project starts with two models of the voting rule, `vote` on the CPU and `gpu_vote`, its port to the GPU.
The [first-model tutorials](first-model/game-of-life.md) add models beside them, and you can delete `vote` and `gpu_vote` once you have your own models.
`gpu_vote` can be deleted on its own, while `vote` can only be deleted together with `gpu_vote`, since the port seeds itself by calling `Vote::init`.

## Renaming it

Renaming the package renames the library too, and the `my_model::` paths in `src/main.rs` and `src/bin/my-model-cli.rs` have to be edited to the new name.
A full rebrand renames these:

--8<-- "templates/model-project/README.md:rename"

The product name is the app's window title, and the folder where the native app keeps its settings is named after it.
"My Model" keeps them in `My-Model` under `~/Library/Application Support` on macOS, in `mymodel` under `$XDG_DATA_HOME` or `~/.local/share` on Linux, and in `My Model\data` under `%APPDATA%` on Windows.

## What is in it

```text
my-model/
├── Cargo.toml
├── README.md
├── build.rs                   # shader bindings and the commit stamp
├── rust-toolchain.toml        # the stable toolchain Henad pins, with the wasm32 target
├── rustfmt.toml               # the 120 columns Henad formats at
├── .gitignore
├── .gitattributes             # LF line ends for the scripts and the pins
├── .cargo/config.toml         # the wasm32 flags of the threaded web build
├── index.html                 # the web page, for Trunk
├── Trunk.toml                 # file names, the two headers and the port of the web build
├── assets/icon-256.png
├── specs/vote.toml            # a small sweep over the initial density
├── scripts/
│   ├── build_web.sh           # the web build, on the dated nightly
│   ├── ci.sh                  # the stages CI runs
│   ├── install-lavapipe.sh    # a GPU driver that runs on the CPU, for CI
│   ├── trunk-sha256           # the SHA-256 of the Trunk tarball CI installs, one line
│   ├── trunk-version          # the Trunk release, one line
│   ├── web-checks.sh          # the checks both web stages run first
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

The `.gitignore` lists `/target` and `/dist`, the two folders that a build writes, and the files that Finder, editors, merges and patches leave beside sources.
It never lists `Cargo.lock`.
A project that keeps sweep folders inside its tree adds `/runs` to it.

## Running it

--8<-- "templates/model-project/README.md:run"

Run a model with `--release`.
The dev profile builds the crate holding your kernels at opt-level 1, with overflow checks and debug assertions on, and a model steps several times slower there.
A toy kernel at opt-level 1 ran about 4.7 times slower than at 2, in single runs.
A plain `cargo run --bin my-model-cli -- vote --params` is enough for the commands that step nothing.

The command line has every mode of `henad-cli`, sweeps, searches, shards and exit codes included.
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

Cargo ignores a dependency's profiles, and the published crates carry no profiles.
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

`ShaderBuild::discover("src")` generates Rust bindings for every shader under `src`.
A GPU model added later joins them on the next build, and no build file changes.
[Shaders and bindings](../authoring/shaders.md) covers the bindings.

`stamp_commit` records the commit the crate was built from, whether its sources differed from it, and a hash of the sources.
Every results folder records that build beside each model.
A resume or a merge compares it with the build that runs it and warns when they differ, and the app warns before it opens a run of another build.
Without the stamp the build is treated as unknown, and an uncommitted edit to a model goes unrecorded.
Two unknown builds never count as the same, and every resume warns whether or not the model changed.
A crate without shaders drops the `ShaderBuild` line and `henad::include_shaders!()`, and keeps `build.rs` for the stamp.

The stamp hashes the files under `src` and the manifest.
A file that a model reads at compile time, through `include_bytes!` or `include_str!`, belongs under `src`, or a change to it goes unrecorded.
The stamp follows no symlink to a folder, and sees no file outside `src` that a shader imports.
A change to a shader there reaches the bindings and goes unrecorded.
The app's icon sits in `assets/`, since it changes no result.

A commit reruns the build script, even one that changes no source, once the project sits in a git repository.
The crate holding your kernels then recompiles, and both binaries relink.
Henad's own crates stay built.
A project built before `git init` records no commit until a file under `src` or the manifest changes, or `cargo clean -p my-model` runs.

## Updating Henad

--8<-- "templates/model-project/README.md:update"

Henad's crates require each other at the same version, and `cargo update -p henad` moves them all.

A new minor release can change the API, and the [CHANGELOG](https://github.com/micfong-z/henad/blob/master/CHANGELOG.md) lists each change with what to do about it.
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

Trunk is the other tool it needs, at the release that `scripts/trunk-version` specifies, and the script prints this command as well when Trunk is missing:

```bash
cargo install --locked trunk --version "$(cat scripts/trunk-version)"
```

The wasm32 flags live in `.cargo/config.toml`, and any flag you add for the web goes into its array.
An exported `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` would replace that array without notice, and the build would fail on an error that mentions neither variable.
The script exits with an error while either variable is set, even to an empty value, and so does the `lint-web` stage of `scripts/ci.sh`.

A host serving `dist/` has to send two headers:

```text
Cross-Origin-Opener-Policy: same-origin
Cross-Origin-Embedder-Policy: require-corp
```

Without them the app runs on one thread in Chrome, and does not start in a browser that blocks shared memory.
GitHub Pages cannot send them, and a site there needs a service worker such as [coi-serviceworker](https://github.com/gzuidhof/coi-serviceworker) to add them.
A project site on GitHub Pages sits under `/<repo>/`, and builds with `scripts/build_web.sh build --release --public-url /<repo>/`.
A browser keeps one web app's saved settings per origin.
Serve each app from its own origin.

## Checks and CI

`scripts/ci.sh` runs the stage given as its argument, or all four stages when it has no argument:

| Stage | Runs |
|---|---|
| `lint` | Clippy over the package, warnings denied |
| `lint-web` | Clippy for the threaded web build, on the nightly |
| `test` | The tests, the check of every model in `models()` among them |
| `web` | The release web build, then a check that `dist/` holds the worker glue |

Each stage passes `--locked` once a `Cargo.lock` exists.
`.github/workflows/ci.yml` runs them on every push to `main`, on every pull request, and when started by hand.
It runs each script through `bash`, since a project committed from Windows records no executable bits.
On Windows, `git add --chmod=+x scripts/*.sh` records them for collaborators on Linux and macOS.

The test at the foot of `src/lib.rs` runs Henad's [testing kit](../authoring/testing.md) over every model in `models()`, and a model added there is checked by the next `cargo test`.
A runner on GitHub has no GPU, and the workflow installs lavapipe, a Vulkan driver that runs on the CPU, through `scripts/install-lavapipe.sh`.
It sets `HENAD_REQUIRE_GPU=1`, and a GPU check that would be skipped fails instead.
Lavapipe has no watchdog.
Run `cargo test` on your own machine's GPU as well.

`cargo deny` reports two crates as unmaintained.
`fxhash`, under RUSTSEC-2025-0057, reaches your project as a build dependency of henad-build's shader generator, and never runs in a binary.
`paste`, under RUSTSEC-2024-0436, is a procedural macro that henad-app's `egui_dock` uses, and goes with the `app` feature.
Henad's own `deny.toml` ignores both advisories with these reasons.

## Publishing a model library

The template is already the shape of a library: the app and the command line sit behind the package's own features `app` and `cli`, both on by default.
To publish your models as a crate:

1. Remove `publish = false` from `Cargo.toml`.
2. Set `license` and `description`, and an `exclude` list for what only the project needs: `index.html`, `scripts/`, `.cargo/` and `specs/`.
3. Keep the `app` and `cli` features, so that a consumer can turn both off.
4. Keep `build.rs` and its `stamp_commit()` line.

A consumer depends on the library with its features off, and adds the models to its own set:

```toml
[dependencies]
my-model = { version = "0.1", default-features = false }
```

```rust
let mut models = henad::ModelSet::new(henad::build_info!());
models.extend(my_model::models()?)?;
```

Each entry keeps your crate's name and version as its source, and a consumer's results folder records them.
Without `default-features = false`, every consumer would compile the app and the command line, since Cargo merges the features that every dependent requests.
A model library is compatible with one Henad 0.x at a time, and moves to the next 0.x release together with Henad.

## Next

- [Writing a CPU grid model](first-model/game-of-life.md) adds a model to the project, step by step.
- [Model sets](../authoring/model-sets.md) covers `models()` and the entries in it.
- [Testing your model](../authoring/testing.md) covers the checks the template's test runs.
