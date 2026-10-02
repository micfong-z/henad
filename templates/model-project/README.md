# My Model

Agent-based models built on [Henad](https://github.com/micfong-z/henad), with their own app, command line and web build.
The project starts with two models of the voting rule: `vote` on the CPU and `gpu_vote`, its port to the GPU.

## Getting the template

The template is fetched from the release it belongs to, and requires that release's crates:

<!-- --8<-- [start:fetch] -->
```bash
mkdir my-model && cd my-model
curl -L https://github.com/micfong-z/henad/archive/refs/tags/v0.3.0.tar.gz \
  | tar -xz --strip-components=3 henad-0.3.0/templates/model-project
```
<!-- --8<-- [end:fetch] -->

## Renaming the project

The package can be renamed alone, and every command keeps working.
Only the `my_model::` paths follow the library's name.
A full rebrand renames these:

<!-- --8<-- [start:rename] -->
- the package name in `Cargo.toml`, and the `my_model::models()` paths in `src/main.rs` and `src/bin/my-model-cli.rs`,
- the two `[[bin]]` names, `default-run`, and the CLI binary's file,
- `data-bin` and `<title>` in `index.html`,
- `cli_command` in `src/main.rs` and `command_name` in the CLI binary,
- the product name passed to `AppOptions::new`.
<!-- --8<-- [end:rename] -->

The product name is the app's window title, and names the folder where the native app keeps its settings, with each space turned into a dash.
"My Model" keeps its settings in a folder named `My-Model`.

## Running

<!-- --8<-- [start:run] -->
```bash
cargo run --release                                         # the app
cargo run --release -- --open runs/vote                     # the app, on a results folder
cargo run --release --bin my-model-cli -- vote --steps 500 --reps 3
cargo run --release --bin my-model-cli -- --spec specs/vote.toml --out runs/vote
```
<!-- --8<-- [end:run] -->

Run a model with `--release`.
The dev profile builds this crate at opt-level 1, with overflow checks and debug assertions on, and a model steps several times slower there.
A plain `cargo run --bin my-model-cli -- vote --params` is enough for the commands that step nothing.

The first build writes `Cargo.lock`.
Commit it, and pass `--locked` to the builds that run sweep shards and resumes, so that every part of a sweep runs the same dependencies.

## Adding a model

A CPU model is a file under `src/`, a `mod` line in `src/lib.rs`, and an `insert` line in `models()`.
A GPU model is a directory under `src/` with its `mod.rs` and its WGSL files, and the same two lines.
The build script finds every shader under `src/` itself, and no build file changes.

`cargo test` checks every model in `models()`: its declarations, a build, its determinism across thread counts and seeds, and, where a GPU exists, its GPU passes.
`HENAD_REQUIRE_GPU=1 cargo test` fails where it would skip a GPU check.
CI installs a GPU driver that runs on the CPU for that.
That driver has no watchdog, so run `cargo test` on your own machine as well.

A crate without shaders drops the `ShaderBuild` line from `build.rs` and `henad::include_shaders!()` from `src/lib.rs`.
It keeps `build.rs` and its `stamp_commit()` line, which records the models' build in every results folder.

## The web build

```bash
scripts/build_web.sh serve --release      # serves on http://127.0.0.1:8081
scripts/build_web.sh build --release      # writes dist/
```

The web build runs on the dated nightly that `scripts/web-toolchain` names, and the script prints the command that installs it when it is missing.
Its flags live in `.cargo/config.toml`.
The script refuses to run while `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` is set, since either would replace them, and a wasm-only flag of your own goes into that file.
A host serving `dist/` sends the `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp` headers, or the app runs on one thread.
Two apps served from one origin share the web app's saved settings.

## Checks

`scripts/ci.sh` runs the stages CI runs: `lint`, `lint-web`, `test` and `web`, or all four without an argument.
`.github/workflows/ci.yml` runs them on every push and pull request, and `.github/dependabot.yml` keeps the dependencies current.

## Updating Henad

<!-- --8<-- [start:update] -->
```bash
cargo update -p henad     # moves henad, henad-build and the crates between them together
cargo update              # every dependency
```

Never update `henad-build` alone with `cargo update -p henad-build`, which leaves the engine behind it.
A new minor release, such as 0.3 to 0.4, changes the `henad` and `henad-build` requirements in `Cargo.toml` together.
Dependabot opens one pull request for both.
<!-- --8<-- [end:update] -->

See [Henad's documentation](https://micfong-z.github.io/henad/) for the guide and the reference.
