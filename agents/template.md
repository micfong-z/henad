# The template

**The template** (`templates/model-project`, package `my-model`, `publish = false`): the project a
user fetches from a release tag and builds on the published crates, outside the workspace through
the root `exclude`. It depends on `henad` and `henad-build` alone among Henad's crates, at the
workspace's major and minor, with the app and the CLI behind its own default features `app` and
`cli`.
Its files are under CC0 1.0, a public-domain dedication, apart from `assets/icon-256.png`, Henad's
icon. The `licence` region of `README.md` states both.
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
lavapipe. `.gitattributes` checks every text file out at LF line ends, Windows included, so the
embedded WGSL and the code hash do not depend on the system, and names the scripts and the pins as
well.
`scripts/install-lavapipe.sh` holds the Mesa and gfx-rs/ci-build pins, and Henad's own `test` job
calls it. `.github/dependabot.yml` groups `henad` and `henad-build`. `README.md` holds the `fetch`,
`licence`, `rename`, `run` and `update` regions, `Cargo.toml` the `profile` region, and
`guide/your-project.md` and `guide/library.md` include them. The template ships no `Cargo.lock`,
and its `.gitignore` never lists one. `rustfmt.toml` sets the 120 columns Henad formats at. Its
`[profile.release]` equals the root's, which `scripts/check_packaging.sh` checks, and `./check.sh`
never builds it.
The `fetch` region runs `git init -b main`. The push trigger of the template's `ci.yml` names `main`
alone, and a bare `git init` names the branch `master` where git's `init.defaultBranch` is unset.
