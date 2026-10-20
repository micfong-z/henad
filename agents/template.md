# The template

**The template** (`templates/model-project`, package `my-model`, `publish = false`): the project a
user fetches from a release tag and builds on the published crates, outside the workspace through
the root `exclude`. It depends on `henad` and `henad-build`, and on `henad-dylib` in its native
target table behind its `hot` feature, all at the workspace's major and minor (`"0.3"`), with the
app and the CLI behind its own default features `app` and `cli`.
`hot = ["app", "henad/hot", "dep:henad-dylib"]` is the dev host, run as
`cargo run --profile hot --features hot`, and `inspection = ["app", "henad/inspection"]` the app's
inspection port. `src/hot.rs` (`use henad_dylib as _;` and
`henad::export_models!(crate::models);`) sits behind one
`#[cfg(all(feature = "hot", not(target_arch = "wasm32")))] mod hot;` line in `src/lib.rs`, and
`src/main.rs` creates the dev host first in `main` and passes it to `hot_reload` under the
feature, and a `hot` build keeps its console window on Windows. `[profile.hot]` (region `hot`)
equals hot-host's. `src/vote.rs` (`Vote`, a `GridModel`) and `src/gpu_vote/` (`GpuVote`, its
`GpuGridModel` port seeded through `Vote::init`) follow the tutorial's import shape, and
`models()` (`src/lib.rs`) inserts each on a line of its own. The test at the foot of `lib.rs` runs
`assert_set_conforms` over `models()` and names no model. `src/main.rs` and
`src/bin/my-model-cli.rs` are the app and the CLI over `henad::app` and `henad::cli`, under the
product name "My Model". `build.rs` calls `stamp_commit` and `ShaderBuild::discover("src")`, and
ends in `Ok(())`, so a crate without shaders deletes one line. `.cargo/config.toml` holds the
wasm32 rustflags. `scripts/web-checks.sh`, which `scripts/build_web.sh` and the `lint-web` stage
source, refuses an inherited `RUSTFLAGS` or `CARGO_ENCODED_RUSTFLAGS` rather than drop them, and
prints the install line of a missing nightly. `scripts/ci.sh` runs the stages `lint`, `lint-web`,
`test` and `web` (all four without an argument), and `hot`
(`HENAD_HOT_CHECK=1 cargo run --profile hot --features hot`) only when named. `lint`, `lint-web`
and `test` take the features in its `features` array, every feature but `hot` and `inspection`, so
they build Henad linked as it ships. Each stage passes `--locked` once a lock exists, and
`.github/workflows/ci.yml` runs them through `bash`, since a checkout from Windows keeps no
executable bit, under `HENAD_REQUIRE_GPU=1` on lavapipe. `.gitattributes` keeps the scripts and
the pins at LF line ends. `scripts/install-lavapipe.sh` holds the Mesa and gfx-rs/ci-build pins,
and Henad's own `test` job calls it. `.github/dependabot.yml` groups `henad`, `henad-build` and
`henad-dylib`. `README.md` holds the `fetch`, `rename`, `run`, `dev-host` and `update` regions,
`Cargo.toml` the `profile` and `hot` regions, and `guide/your-project.md`, `guide/library.md` and
`guide/hot-reload.md` include them. `cargo update -p henad -p henad-dylib` moves the Henad crates:
henad-dylib's exact requirement holds the facade back under `-p henad` alone, which then moves
nothing. The template ships no `Cargo.lock`, and its `.gitignore` never lists one. `rustfmt.toml`
sets the 120 columns Henad formats at. Its `[profile.release]` equals the root's, which
`scripts/check_packaging.sh` checks, and `./check.sh` never builds it.
