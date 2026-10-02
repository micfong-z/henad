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
Each requires its siblings at that version with a caret, as in `henad-core = "0.3.0"`, and `scripts/check_packaging.sh` holds every requirement equal to the workspace version.
A crate then never resolves against a sibling older than itself.

A downstream crate requires `henad` and `henad-build` as `"0.3"`, and takes a patch release through `cargo update -p henad`.
The tutorial crate, `examples/tutorial`, sets `publish = false` and never publishes.

## The release checklist

Copy the checklist into an issue titled "Release 0.x.y" for each release, and tick it off on the commit to be tagged, once that commit is pushed.

```markdown
- [ ] `[workspace.package]` carries the release's version, and so does every `henad-*` entry of `[workspace.dependencies]`.
- [ ] `templates/model-project/Cargo.toml` requires the release's major and minor in all three places: `henad` and `henad-build`, and the `henad` line of `[dev-dependencies]`.
- [ ] `docs/license.html` is regenerated, and the `lint` job passes on it.
- [ ] The CHANGELOG section of the release is dated, and `python3 scripts/changelog_section.py 0.x.y` prints the notes.
- [ ] The `fetch` region of `templates/model-project/README.md` names the tag about to be made.
- [ ] `templates/model-project/Cargo.lock` does not exist.
- [ ] Verified packaging, default features, in a fresh target directory:
      `CARGO_TARGET_DIR="$(mktemp -d)" cargo package --workspace --exclude henad-tutorial --locked`
- [ ] Verified packaging of the facade's features, in a fresh target directory of its own:
      `CARGO_TARGET_DIR="$(mktemp -d)" cargo package --workspace --exclude henad-tutorial --locked --features henad/example-models,henad/app,henad/cli,henad/testing`
- [ ] The `downstream` job passes on this commit: `gh workflow run ci.yml --ref <branch>`
- [ ] Patch release only: `cargo semver-checks` over the published crates against the previous release.
- [ ] Patch release only: `git diff v0.x.(y-1) -- 'crates/henad-core/src/**/*.wgsl'` changes no import path, item name, signature, struct layout or `WORKGROUP`.
- [ ] `cargo publish --workspace --dry-run --locked`
- [ ] `master` is green on CI.
```

The two packaging passes build every crate from its tarball, against the other crates' tarballs, before anything is uploaded.
`cargo package` verifies default features alone, and the facade has none, so the second pass builds its gated items.
Each pass takes a fresh `CARGO_TARGET_DIR`.
Cargo extracts its overlay of unpublished crates under `~/.cargo/registry/src/`, at a path set by the target directory, and keeps an earlier run's extraction of the same version.
A verify in the usual target directory can then build the new crates against an old henad-core.

The `downstream` job copies the template outside the checkout, points it at the packaged crates, and runs what a user of the template runs: the strict lint, the tests with a GPU required, the web build and its checks, a rebuild after a model edit, and a sweep with a resume.
It runs on demand only, and the checklist starts it.
To run its steps by hand, run each step's script from `.github/workflows/ci.yml` with `RUNNER_TEMP` set to a directory outside any git work tree.

`cargo semver-checks` cannot see WGSL.
The shared modules under `crates/henad-core/src/` are an interface of their own, and the diff review applies the rule of [stability](#stability) to them.

## Cutting a release

1. **Bump the version.**
   Set `[workspace.package] version`, every `henad-*` entry of `[workspace.dependencies]`, and the template's three requirements in `templates/model-project/Cargo.toml`: `henad` and `henad-build`, and `henad` under `[dev-dependencies]`.
   Set the tag in the `fetch` region of `templates/model-project/README.md` as well.
   `scripts/check_packaging.sh` holds the workspace requirements to the version.

2. **Regenerate the licence page.**
   The page lists every crate with its version, and the `lint` job fails on a stale one.

    ```bash
    cargo about generate about.hbs -o docs/license.html
    ```

3. **Refresh `Cargo.lock`**, which records the workspace's own versions.

    ```bash
    cargo update --workspace
    ```

4. **Date the version in `CHANGELOG.md`.**
   `## [Unreleased]` becomes `## [0.3.0] - 2026-10-20`.
   Open a fresh `## [Unreleased]` above it, and repoint the link definitions at the foot of the file.

5. **Read the notes before anyone else does.**

    ```bash
    python3 scripts/changelog_section.py 0.3.0
    ```

    This prints exactly what the release body will be.

6. **Commit the bump and push it.**

7. **Run the [checklist](#the-release-checklist) on that commit.**
   CI does not run on tags, so a draft release is no evidence the tree builds.

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
crates.io allows five new crates in a burst and then one every ten minutes, and the first release creates eight:

```bash
cargo publish --workspace --dry-run --locked
cargo publish --locked -p henad-core -p henad-build -p henad-compute -p henad-models -p henad-explore
# ten minutes later
cargo publish --locked -p henad-cli
# ten minutes later
cargo publish --locked -p henad-app
# ten minutes later
cargo publish --locked -p henad
```

henad-models comes before the two hosts, whose optional dependency on it has to resolve.
A rate limit hit part way leaves the earlier crates published, and the run resumes with `-p` for the rest.
`cargo publish --workspace` skips the tutorial crate.

A later release adds versions to existing crates, and crates.io allows thirty of those in a burst:

```bash
cargo publish --workspace --locked
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
  The error names the versions the file does have.
- **The section carries no date.**
  `## [0.3.0]` with no date is a section still being written, not a release.
  `## [Unreleased]` fails earlier, as no section for the version.

A tag with a pre-release suffix, `v0.3.0-rc.1`, is marked as a pre-release on GitHub.
A candidate tag needs the workspace, every requirement and a dated section at the candidate's version.
A candidate tag is no rehearsal of a release.

## What it does not do

No binaries are built or attached.
Users install the app and the command line from crates.io, at the opt-level every Henad number is measured at:

```bash
cargo install --locked --config 'profile.release.opt-level=2' henad-app henad-cli
```

`cargo install` builds a package as the root, and a package carries no profiles.
Without the `--config` it would build at Cargo's default of 3.

The site and the web app deploy from `master`, never from a tag.
The docs workflow publishes the site on every push to `master`.
The web app is a Vercel project whose production deploys from `master`, with the install and build commands in `vercel.json`: the dated nightly and Trunk at the releases `templates/model-project/scripts/` names, then `./scripts/build_web.sh build --release`.
The guide includes its code from the template, the tutorial crate and the facade's example, and between releases it can show calls the published crates lack.
Every page that includes template or tutorial code states the Henad version it describes.

## Stability

A **breaking release** goes from 0.x to 0.(x+1), and only there may an item break.
A **patch release** goes from 0.x.y to 0.x.(y+1) and stays compatible, as Cargo's rule for 0.x versions requires.

- Every breaking release may break the API.
  Keep a Changelog 1.1, the format CHANGELOG.md follows, has no "Breaking" heading.
  Each break goes under Changed or Removed with a leading "Breaking:" and a migration note.
- A replacement ships at least one breaking release before the item it replaces goes, with `#[deprecated(since, note)]` in between.
- Items under `#[doc(hidden)]` sit outside the documented surface, and serve Henad's own crates alone: `__shader_support`, `__macro_support`, the `build_info!` constructor, `__COMPUTE_BUILD`, the hidden version consts, henad-build's `stamp_engine_commit` and `stamp_source_hash`, and `ModelEntry::wrap_factory` with its `Factory` trait.
  Under caret requirements Henad's crates can meet at different patch releases.
  Within one 0.x no crate therefore removes or changes a hidden item another Henad crate of that 0.x uses, a `HENAD_BUILD_*` variable name, or the shape of henad-build's generated code.
  `wrap_factory` and `Factory` stay fully exempt, since only Henad's tests call them.
- The shared WGSL has an interface of its own.
  A change to a shared module's import path, item names, function signatures, struct layouts or `WORKGROUP`, or to a hand-written Rust mirror of one, ships only in a breaking release.
  A fix to a function body can ship in a patch release, and changes `SHARED_WGSL_FNV1A64` with it.
- The major versions of the re-exported `wgpu` and `bytemuck` are part of Henad's API, and a new major of either is a breaking release.
- Types whose fields authors fill by struct literal gain fields in a breaking release only: `BufferSpec`, `PassSpec`, `BindingDecl`, `SpringParams`, `Extent`, `LaneSpec`, `GpuGridAction`, `GpuAgentAction`, `ReduceSpec` and `DisplaySpec`.
  The enums authors pick or match on, `Domain`, `Boundary`, `NeighborhoodKind` and `PassId`, gain variants in a breaking release only.
- `ParamDescriptor`, `StatDescriptor`, `StatEntry` and `ActionDescriptor` keep public `&'static` fields in 0.3.
  A later breaking release makes them private and owned, with `ModelMetadata` and the details of `Structure`.
  Model code builds the descriptors through helpers and `const fn new`, and no model file changes with them.
  The spec types (`SweepSpec`, `BlockSpec`, `RunSettings`, `MeasureSettings`, `SeedSettings`, `ActionSpec`, `FactorSpec` and the rest) stay exhaustive in 0.3, since hosts and tests build them by literal.
- henad-models' items are the example models' own, and may change in any breaking release.
- `ParamValue` converts from `f32`, `u32` and `bool` alone.
  Another `From` impl would break the unsuffixed literals callers pass.
- A struct with public fields gains a field, and an enum gains a variant, in a breaking release only.
  That covers the enums the engine produces and hosts match on, such as `ProgressEvent`, `SweepEvent`, `SweepWarning`, `FaultKind`, `RunStatus`, `StopReason` and `ModelState`, and the output structs such as `SweepRecord`, `SweepReport`, `ResultCounts` and `Manifest`.
- `#[non_exhaustive]` marks the types built through `new`: `SweepOptions`, `SweepRunOptions`, `BenchmarkSettings`, `TestDeviceRequest` and `SimulationViews`.
  It also marks the types that are lists by nature: the error enums `SetupError`, `ExportError`, `ModelSetError` and `ModelLookupError`, `AppOpening` and `ModelCheck`.
- The output of `henad-cli --list`, `--params` and `--params --json`, the benchmark's `--json` lines, the spec TOML, `manifest.json` and the CSV tables are a contract of their own, changed only with a note in the CHANGELOG.
  A new optional field is additive.

Versions move in lockstep with caret requirements.
One version per release keeps every combination a user can resolve one that Henad has built and tested together, and a model library is compatible with one Henad 0.x at a time.
