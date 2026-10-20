# The template

**The template** (`templates/model-project`, package `my-model`, `publish = false`): the project a
user fetches from a release tag and builds on the published crates, outside the workspace through
the root `exclude`. It depends on `henad` and `henad-build` alone, at the workspace's major and
minor (`"0.3"`), with the app and the CLI behind its own default features `app` and `cli`.
`src/vote.rs` (`Vote`, a `GridModel`) and `src/gpu_vote/` (`GpuVote`, its `GpuGridModel` port
seeded through `Vote::init`) follow the tutorial's import shape, and `models()` (`src/lib.rs`)
inserts each on a line of its own. The test at the foot of `lib.rs` runs `assert_set_conforms`
over `models()` and names no model. `src/main.rs` and `src/bin/my-model-cli.rs` are the app and
the CLI over `henad::app` and `henad::cli`, under the product name "My Model". `build.rs` calls
`stamp_commit` and `ShaderBuild::discover("src")`, and ends in `Ok(())`, so a crate without
shaders deletes one line. `.cargo/config.toml` holds the wasm32 rustflags. `scripts/web-checks.sh`,
which `scripts/build_web.sh` and the `lint-web` stage source, refuses an inherited `RUSTFLAGS` or
`CARGO_ENCODED_RUSTFLAGS` rather than drop them, and prints the install line of a missing nightly.
`scripts/ci.sh` runs the stages `lint`, `lint-web`, `test` and `web` (all four without an
argument), with `--locked` once a lock exists, and `.github/workflows/ci.yml` runs them through
`bash`, since a checkout from Windows keeps no executable bit, under `HENAD_REQUIRE_GPU=1` on
lavapipe. `.gitattributes` keeps the scripts and the pins at LF line ends.
`scripts/install-lavapipe.sh` holds the Mesa and gfx-rs/ci-build pins, and Henad's own `test` job
calls it. `.github/dependabot.yml` groups `henad` and `henad-build`. `README.md` holds the `fetch`,
`rename`, `run` and `update` regions, `Cargo.toml` the `profile` region, and
`guide/your-project.md` and `guide/library.md` include them. The template ships no `Cargo.lock`,
and its `.gitignore` never lists one. `rustfmt.toml` sets the 120 columns Henad formats at. Its
`[profile.release]` equals the root's, which `scripts/check_packaging.sh` checks, and `./check.sh`
never builds it.
