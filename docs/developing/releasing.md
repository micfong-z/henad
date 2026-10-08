---
title: Releasing
description: How a version is cut and published to crates.io, the checklist each release runs, and what a release may change.
icon: material/tag-outline
---

# Releasing

A release is a git tag on `master` and eight crates on crates.io.
Pushing `vX.Y.Z` drafts a GitHub release whose body is that version's `CHANGELOG.md` section, and the maintainer publishes the crates by hand.
Nothing is published without a human running the command or pressing the button.

## Versions

Eight crates publish: henad-core, henad-build, henad-compute, henad-models, henad-explore, henad-cli, henad-app and henad.
All eight share the version in `[workspace.package]`, and every release moves them together.
Each requires its siblings at that version with a caret, as in `henad-core = "0.3.0"`, and `scripts/check_packaging.sh` checks that every requirement equals the workspace version.
A crate then never resolves against a sibling older than itself.

A downstream crate requires `henad` and `henad-build` as `"0.3"`, and picks up a patch release with `cargo update -p henad`.
The tutorial crate, `examples/tutorial`, sets `publish = false` and never publishes.

## The release checklist

Copy the checklist into an issue titled "Release 0.x.y" for each release, and tick it off on the commit to be tagged, once that commit is pushed.

```markdown
- [ ] `[workspace.package]` carries the release's version, and so does every `henad-*` entry of `[workspace.dependencies]`.
- [ ] The root `pyproject.toml` and `uv.lock` carry the release's version.
- [ ] `templates/model-project/Cargo.toml` requires the release's major and minor in all three places: `henad` and `henad-build`, and the `henad` line of `[dev-dependencies]`.
- [ ] `docs/license.html` is regenerated, and the `lint` job passes on it.
- [ ] The CHANGELOG section of the release is dated, and `python3 scripts/changelog_section.py 0.x.y` prints the notes.
- [ ] The `fetch` region of `templates/model-project/README.md` names the tag about to be made.
- [ ] Breaking release only: the `!!! info "Henad 0.x"` admonitions and the dependency snippets typed into the guide name the release's major and minor.
- [ ] `templates/model-project/Cargo.lock` does not exist.
- [ ] Verified packaging, default features, in a fresh target directory:
      `CARGO_TARGET_DIR="$(mktemp -d)" cargo package --workspace --exclude henad-tutorial --locked`
- [ ] Verified packaging of the facade's features, in a fresh target directory of its own:
      `CARGO_TARGET_DIR="$(mktemp -d)" cargo package --workspace --exclude henad-tutorial --locked --features henad/example-models,henad/app,henad/cli,henad/testing`
- [ ] The dispatched run passes on this commit, its `downstream`, `msrv`, `features` and `docs` jobs included: `gh workflow run ci.yml --ref <branch>`
- [ ] A native check on the pinned nightly prints no future-incompatibility warning for a Henad crate, since docs.rs builds on nightly, and CI runs that nightly only to lint wasm32 and to build each crate's documentation through `scripts/docs_rs.py`:
      `CARGO_TARGET_DIR="$(mktemp -d)" cargo +"$(cat templates/model-project/scripts/web-toolchain)" check --workspace --all-targets --all-features`
- [ ] The facade's rendered `henad::params` page is its own module, listing `ValueError` and `format_value`, and not henad-core's `params` module with `ParamStore`:
      `cargo doc --locked -p henad --no-deps --all-features && grep -c 'ValueError' target/doc/henad/params/index.html`
- [ ] Patch release only: `cargo semver-checks` over the published crates against the previous release.
- [ ] Patch release only: `git diff v0.x.(y-1) -- 'crates/henad-core/src/**/*.wgsl'` changes no import path, item name, signature, struct layout or `WORKGROUP`.
- [ ] Patch release only: `git diff v0.x.(y-1) -- crates` changes no `#[doc(hidden)]` item that another Henad crate uses.
      `git grep -n 'doc(hidden)' -- crates` lists the hidden items.
- [ ] `CARGO_TARGET_DIR="$(mktemp -d)" cargo publish --workspace --dry-run --locked`
- [ ] `master` is green on CI.
```

The two packaging passes build every crate from its tarball, against the other crates' tarballs, before anything is uploaded.
`cargo package` verifies default features alone, and the facade has no default features, so the second pass builds its gated items.
Each pass uses a fresh `CARGO_TARGET_DIR`, and so do the publish dry run and every publish.
Cargo extracts its overlay of unpublished crates under `~/.cargo/registry/src/`, at a path set by the target directory, and keeps an earlier run's extraction of the same version.
A verify in the usual target directory can then build the new crates against an old henad-core.

The `downstream` job copies the template outside the checkout, points it at the packaged crates, and runs what a user of the template runs: the strict lint, the tests with a GPU required, the web build and its checks, a rebuild after a model edit, and a sweep with a resume.
It runs on demand only, and the checklist starts it.
The same dispatch runs the `msrv`, `features` and `docs` jobs, and a push to `master` skips them.
To run its steps by hand, run each step's script from `.github/workflows/ci.yml` with `RUNNER_TEMP` set to a directory outside any git work tree and `CARGO_TERM_COLOR=never`, as the job does.
Two of its steps grep cargo's log, and a colour code splits the words they match.

`cargo semver-checks` cannot see WGSL.
The shared modules under `crates/henad-core/src/` form a separate interface, and the diff review applies the [stability](#stability) rules to them.
`cargo semver-checks` does not see `#[doc(hidden)]` items either, and the hidden items that one Henad crate calls in another form a contract of the same kind.

## Cutting a release

1. **Bump the version.**
   Set `[workspace.package] version`, every `henad-*` entry of `[workspace.dependencies]`, and the template's three requirements in `templates/model-project/Cargo.toml`: `henad` and `henad-build`, and `henad` under `[dev-dependencies]`.
   Set the tag in the `fetch` region of `templates/model-project/README.md` as well.
   A breaking release also sets its major and minor in the `// Cargo.toml:` line that opens both `crates/henad/README.md` and `crates/henad/examples/complete.rs`, in the `!!! info "Henad 0.x"` admonitions, and in the dependency snippets typed into `docs/guide/library.md`, `docs/authoring/testing.md` and `docs/reference/cli.md`.
   `git grep -nE 'Henad 0\.[0-9]+|version = "0\.[0-9]+"' -- docs crates/henad ':!docs/developing/agent-record'` lists the lines to check.
   `scripts/check_packaging.sh` checks that the workspace requirements match the version, and that the template's three requirements and the README's dependency line match its major and minor.
   Set `version` in the root `pyproject.toml`, the documentation site's project, and run `uv lock`.

2. **Regenerate the licence page.**
   The page lists every crate with its version, and the `lint` job fails on a stale page.

    ```bash
    cargo about generate about.hbs -o docs/license.html
    ```

3. **Refresh `Cargo.lock`**, which records the workspace's own versions.

    ```bash
    cargo update --workspace
    ```

4. **Date the version in `CHANGELOG.md`.**
   The release's notes sit under its own heading below an empty `## [Unreleased]`, and that heading takes the date, `## [0.3.0]` becoming `## [0.3.0] - 2026-10-20`.
   Notes still under `## [Unreleased]` move below the release's heading first.
   A release without a heading yet opens one right below `## [Unreleased]`.
   Repoint the link definitions at the foot of the file.

5. **Read the notes before anyone else does.**

    ```bash
    python3 scripts/changelog_section.py 0.3.0
    ```

    This prints exactly what the release body will be.

6. **Commit the bump and push it.**

7. **Run the [checklist](#the-release-checklist) on that commit.**
   CI does not run on tags, so a draft release does not show that the tree builds.

8. **Tag and push the tag.**

    ```bash
    git tag v0.3.0
    git push origin v0.3.0
    ```

9. **Publish the crates**, as [below](#publishing-to-cratesio).

10. **Read the draft and publish it.**

11. **Check the deployed web app.**
    Its About window shows the released version once `master` has deployed.

## Publishing to crates.io

The crates publish in dependency order.
crates.io allows five new crates in a burst and then one every ten minutes, and the first release creates eight crates:

```bash
CARGO_TARGET_DIR="$(mktemp -d)" cargo publish --workspace --dry-run --locked
CARGO_TARGET_DIR="$(mktemp -d)" cargo publish --locked -p henad-core -p henad-build -p henad-compute -p henad-models -p henad-explore
# ten minutes later
CARGO_TARGET_DIR="$(mktemp -d)" cargo publish --locked -p henad-cli
# ten minutes later
CARGO_TARGET_DIR="$(mktemp -d)" cargo publish --locked -p henad-app
# ten minutes later
CARGO_TARGET_DIR="$(mktemp -d)" cargo publish --locked -p henad
```

Each command verifies in a fresh target directory, as the packaging passes do.
henad-models comes before the two hosts, whose optional dependency on it has to resolve.
A rate limit hit part way leaves the earlier crates published, and the run resumes with `-p` for the remaining crates.
`cargo publish --workspace` skips the tutorial crate.

A later release adds versions to existing crates, and crates.io allows thirty new versions in a burst:

```bash
CARGO_TARGET_DIR="$(mktemp -d)" cargo publish --workspace --locked
```

Cargo 1.97 or later is required.
Cargo 1.96 raised a false deadlock error while waiting for the registry.

A broken release is yanked, and a patch release of all eight follows.

### The crates.io account

- crates.io requires a verified email before any publish, and the maintainer's account has one.
- The first release publishes with a token scoped to `publish-new`, since it creates eight crates.
  Every later release publishes with a token scoped to `publish-update`.
  Each token is limited to the `henad*` crates and carries an expiry.
- The maintainer is the sole owner of every crate.

The tag workflow never uploads.
Trusted publishing from a tag, through `rust-lang/crates-io-auth-action`, stays an option should the step ever be automated.

## What the tag build refuses

Three ways it stops rather than announcing something wrong:

- **The tag and the manifest disagree.**
  `v0.3.0` against a workspace still at `0.2.0` fails before anything is created.
- **The changelog has no section for the version.**
  The error lists the versions that the file does have.
- **The section carries no date.**
  `## [0.3.0]` with no date is a section still being written, not a release.
  `## [Unreleased]` fails at the previous check, since the file then has no section for the version.

A tag with a pre-release suffix, `v0.3.0-rc.1`, is marked as a pre-release on GitHub.
A candidate tag needs the workspace, every requirement and a dated section at the candidate's version.
A candidate tag is not a rehearsal of a release.

## What it does not do

No binaries are built or attached.
Users install the app and the command line from crates.io, at the opt-level every Henad number is measured at:

```bash
cargo install --locked --config profile.release.opt-level=2 henad-app henad-cli
```

`cargo install` builds a package as the root, and a package carries no profiles.
Without the `--config` it would build at Cargo's default of 3.

The site and the web app deploy from `master`, never from a tag.
The docs workflow publishes the site on every push to `master`.
The web app is a Vercel project whose production deploys from `master`, with the install and build commands in `vercel.json`: the dated nightly and Trunk at the releases pinned in `templates/model-project/scripts/`, then `./scripts/build_web.sh build --release`.
The guide includes its code from the template, the tutorial crate and the facade's example, and between releases it can show calls that the published crates lack.
Every page that includes template or tutorial code states the Henad version it describes.

## Stability

A **breaking release** goes from 0.x to 0.(x+1), and an item can break only in a breaking release.
A **patch release** goes from 0.x.y to 0.x.(y+1) and stays compatible, as Cargo's rule for 0.x versions requires.

- Every breaking release may break the API.
  Keep a Changelog 1.1, the format CHANGELOG.md follows, has no "Breaking" heading.
  Each break goes under Changed or Removed with a leading "Breaking:" and a migration note.
- A replacement ships at least one breaking release before the item that it replaces is removed, with `#[deprecated(since, note)]` in between.
- Items under `#[doc(hidden)]` sit outside the documented surface, and serve only Henad's own crates.
  Among them are `__shader_support`, `__macro_support`, the `build_info!` constructor, `__COMPUTE_BUILD`, the hidden version consts, the `__indices!` and `__buffer_flags!` macros, henad-build's `stamp_engine_commit` and `stamp_source_hash`, `ModelEntry::wrap_factory` with its `Factory` trait, and `AppOptions::__official`.
  henad-compute calls the graph maintenance methods of henad-core's `Network` (`spawn`, `retire`, `set_directed`, `should_repack`, `repack` and `rebuild`) and the two hidden constructors of `ModelSource`, `__from_type_path` and `__with_build`.
  Under caret requirements, Henad's crates can resolve to different patch releases in one build.
  Within one 0.x no crate therefore removes or changes a hidden item that another Henad crate of that 0.x uses, a `HENAD_BUILD_*` variable name, or the shape of henad-build's generated code.
  `wrap_factory` and `Factory` stay fully exempt, since only Henad's tests call them.
- The shared WGSL has its own interface.
  A change to a shared module's import path, item names, function signatures, struct layouts or `WORKGROUP`, or to a hand-written Rust mirror of a shared module, ships only in a breaking release.
  A fix to a function body can ship in a patch release, and changes `SHARED_WGSL_FNV1A64` with it.
- The major versions of the re-exported `wgpu` and `bytemuck` are part of Henad's API, and a new major version of either crate is a breaking release.
  So are the majors of the crates whose types appear in a public signature: `toml` and `serde_json` in `SpecFileError`, `LoadedSpec::to_toml` and the `Value` fields of `Manifest`, `rayon` in `ExecutionError::Pool`, `log` in `init_web_logger`, and, for henad-app on wasm32, `wasm-bindgen-rayon`.
- Types whose fields authors fill by struct literal gain fields in a breaking release only: `BufferSpec`, `PassSpec`, `BindingDecl`, `SpringParams`, `Extent`, `LaneSpec`, `GpuGridAction`, `GpuAgentAction`, `ReduceSpec` and `DisplaySpec`.
  The enums that authors pick or match on, `Domain`, `Boundary`, `NeighborhoodKind` and `PassId`, gain variants in a breaking release only.
- `ParamDescriptor`, `StatDescriptor`, `StatEntry` and `ActionDescriptor` keep public `&'static` fields in 0.3.
  A later breaking release makes them private and owned, with `ModelMetadata` and the details of `Structure`.
  Model code builds the descriptors through helpers and `const fn new`, and no model file changes with them.
  The spec types (`SweepSpec`, `BlockSpec`, `RunSettings`, `MeasureSettings`, `SeedSettings`, `ActionSpec`, `FactorSpec` and the rest) stay exhaustive in 0.3, since hosts and tests build them by literal.
- henad-models' items belong to the example models, and may change in any breaking release.
- `ParamValue` converts from `f32`, `u32` and `bool` alone.
  Another `From` impl would break the unsuffixed literals callers pass.
- A struct with public fields gains a field, and an enum gains a variant, in a breaking release only.
  That covers the enums that the engine produces and hosts match on, such as `ProgressEvent`, `SweepEvent`, `SweepWarning`, `RunStatus`, `StopReason` and `ModelState`, and the output structs such as `SweepRecord`, `SweepReport`, `ResultCounts` and `Manifest`.
- `#[non_exhaustive]` marks the types built through `new`: `SweepOptions`, `SweepRunOptions`, `BenchmarkSettings`, `TestDeviceRequest` and `SimulationViews`.
  It also marks the types that are lists by nature: the error enums `SetupError`, `ExportError`, `ModelSetError`, `ModelLookupError` and `ShaderBuildError`, `AppOpening`, `OpenAt`, `ModelCheck` and `SkipReason`.
  `ExploreError` and `SweepOutput` carry it as well, since their variants differ between targets.
  `FaultKind` carries it too, and gains a variant when the engine learns to report a new kind of fault.
  So do the benchmark's `BenchmarkEvent`, `BenchmarkReport` and `RepetitionReport`.
- The output of `henad-cli --list`, `--params` and `--params --json`, the benchmark's `--json` lines, the spec TOML, `manifest.json` and the CSV tables form a separate contract, changed only with a note in the CHANGELOG.
  A new optional field is additive.

Versions move in lockstep with caret requirements.
With one version per release, every combination that a user can resolve is one that Henad has built and tested together, and a model library is compatible with one Henad 0.x at a time.
