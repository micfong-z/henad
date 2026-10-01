---
date: 2026-10-01
title: "Library M5: the shared WGSL and henad-build"
description: The fifth milestone of #48. The shared WGSL moves into henad-core under the import root henad::, a new henad-build crate generates every crate's shader bindings, include_shaders! brings them in, and the grid engine's Dims comes from the generated code. The M4 review is folded in first.
icon: material/package-variant
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#48"
state: M4 review folded in, M5 implemented, `HENAD_REQUIRE_GPU=1 ./check.sh` and the docs build green, every shipped shader identical to 773a7a5's apart from import names, seven crates verified from their tarballs
baseline_commit: cdaa134
delta_state: uncommitted on `48-library`
---

# Library M5: the shared WGSL and henad-build

> M5 is the fifth of the eleven milestones of #48, which turns Henad into a library published on crates.io.
> The session first folded in the maintainer's review of M4, then moved the five shared WGSL modules into henad-core and renamed their import root from `shared::` to `henad::`.
> A new crate, henad-build, runs from the build scripts of henad-compute, henad-models and henad-app, and `include_shaders!` brings the generated code into each.
> henad-models finds its shaders by itself, a binding line in an unread form fails the build, and the grid engine's `Dims` is generated rather than written by hand.
> Every shader the engines compile is byte for byte the shader 773a7a5 compiled, once the import names are mapped.
> A review of M5 before its commit then closed ten findings, from a stale stamp to names the generated code shadows.

## State before

`48-library` stood at cdaa134, M4's commit, with a clean tree.
The shared WGSL sat in `crates/henad-compute/src/gpu/shared/`, imported as `shared::prelude`, `shared::space` and so on, and henad-models' build script read it through `../henad-compute/src/gpu`.
Three build scripts each drove `wgsl_bindgen` by hand, henad-models listed its seventeen entry points in `ENTRY_POINTS`, and only henad-models generated `binding_decls`.
The binding parser kept lines that opened with `@group(0)` and skipped any other without a word.
`henad_compute::shader_bindings` was public, the grid engine's `Dims` was written by hand, and henad-models asserted it against the generated struct.
The maintainer's review of M4 listed five items.

## What was done

### The M4 review

1. `lookup_message` returns the capitalised error with no full stop.
   The four slots that embed it ("Spec load failed: ...", "... start failed: ...", "Resume failed: ..." and the runs table's refusal) stay as they were.
   The first pass added the stop in `open_run`, whose message stands alone in the Results status line, and the M5 review took it out again.
2. `select_model` loads the defaults from `self.models.get(id)`, and an id the set lacks leaves `param_values` and `pending_reload` empty.
   `load_default_params`, whose one caller it was, is folded into it.
3. `ModelSet::runnable(gpu)` lists the entries a host with or without a device can run.
   `lookup` finds its entry through it, and the app's and the CLI's `offered` functions are gone in favour of it. `AppState::offered_models` calls it.
4. The CLI's crate doc is rewrapped within 120 columns, and its `ModelSet` link no longer needs an import.
   `wgpu_configuration` and `device_descriptor` open with "Returns".
5. A GPU model without compute reads "Model 'x' needs a GPU with compute support, and this device has none", and `NO_MODEL_RUNS` ends "GPU models need a GPU with compute support."
   The test, the CHANGELOG, AGENTS.md and the design's [4.4.6] say the same.

### The shared WGSL in henad-core

- `dims`, `rng`, `space` and `reduce_tree` moved by `git mv` into `crates/henad-core/src/authoring/primitives/wgsl/`, and `prelude.wgsl` became `dispatch.wgsl`.
  Each declares `henad::<module>`, and all 31 `#import shared::` lines in henad-compute and henad-models now import `henad::`.
- `wgsl/mod.rs` holds `SharedModule`, `SHARED_WGSL_MODULES` and `SHARED_WGSL_FNV1A64`.
  The hash runs at compile time through `Fnv1a64`, whose `write`, `write_u64`, `write_str` and `finish` became `const fn`.
  Two tests check each module's first line and the const hash against one computed at run time.
- `parity.wgsl` moved to `crates/henad-compute/src/gpu/tests/`, lost its import path, and declares `CODES`, an array referencing `TORUS`, `BOUNDED`, `MOORE_ROW_MAJOR`, `MOORE_COLUMN_MAJOR` and `VON_NEUMANN`.
  All five now appear in `shader_bindings::henad::space`, where the parity test reads them as `codes::*`.

### henad-build

- `ShaderBuild::discover(root)` walks the root and takes every `.wgsl` file without a `#define_import_path` line as an entry point.
  `ShaderBuild::new(root).entry_point(path)` names them one by one, as henad-compute does.
- `paths.rs` refuses a path component that is no ASCII identifier or is a keyword, a `henad.wgsl` file or a `henad` directory holding a `.wgsl` file, and a root named `henad`, each in any case.
  It refuses a first path component the generated code uses at its root: `wgpu`, `bytemuck`, `std`, `core`, `alloc`, `_root`, `ShaderEntry`, `layout_asserts` and `bytemuck_impls`.
  It refuses two entries that give one module path, one `ShaderEntry` variant (computed with heck's `to_pascal_case`, as `wgsl_bindgen` does) or one binding constant, and an entry whose module holds another's.
  A `henad` directory with no `.wgsl` file is allowed, and a `#define_import_path henad::...` line elsewhere draws a `cargo:warning`.
- `generate` copies the shared modules to `OUT_DIR/henad_wgsl/henad/`, hands that folder to `wgsl_bindgen` as a scan directory, and runs it with `emit_rerun_if_change(false)`.
  It prints one `cargo:rerun-if-changed` for the absolute shader root, plus one per entry `new` named, in the single-colon form, which keeps a downstream crate's MSRV below 1.77.
  It skips `wgsl_bindgen` when an FNV-1a hash of its inputs (henad-build's version, the pinned `wgsl_bindgen` release, the source of `output.rs`, the shared hash, the root, the entries and every `.wgsl` file under the root) matches `shader_bindings.stamp`.
  `scripts/check_packaging.sh` holds `WGSL_BINDGEN_VERSION` equal to the workspace's `=0.23.3` pin.
  It writes every file only when its bytes change, and calls `generate_string` itself in place of `wgsl_bindgen`'s own write.
  A crate with no entry points gets an empty `shader_bindings.rs` and a `binding_decls.rs` with the hash and an empty `bindings` module, and its stamp is removed.
- `binding_lines.rs` is the private reader.
  It strips block comments (nested ones included) and line comments, then refuses every line holding `@binding(` or opening with `@group(` that is not `@group(G) @binding(N) var<...> name: Type;` on one line, naming the line and the reason.
  It reads `uniform`, `storage`, `storage, read`, `storage, read_write` and `texture_storage_*`, refuses a sampled texture or a sampler, and skips groups other than 0.
  A module that is no entry point and holds a `@binding(` line fails with `ModuleBinding`, which says bindings belong in the entry shader.
- `output.rs` writes `binding_decls.rs` with `use super::{BindingDecl, BindingKind}` and, for every entry with group-0 bindings, a `const _: () = ::core::assert!` holding its list to `WgpuBindGroup0::LAYOUT_DESCRIPTOR.entries.len()`.
- `wgsl_bindgen` is pinned at `=0.23.3` in the workspace table, and henad-build alone depends on it.
  heck joins the workspace table, already in the lock through `wgsl_bindgen`.
- `ShaderBuildError`'s `Debug` writes its `Display`, and a build script that returns it as `Box<dyn Error>` shows the guidance.
- Thirteen tests in `src/tests/`: the four path tests the design names, `a_path_starting_with_a_generated_name_is_refused`, `an_import_path_under_henad_elsewhere_draws_a_warning`, three reader tests with `a_binding_in_a_module_is_refused`, and `a_second_build_reruns_nothing`, `a_shader_added_between_builds_is_generated`, `a_crate_without_shaders_builds` and `shaders_removed_and_restored_are_generated_again`.
  None runs Cargo. The parser test reads the generated layout's `@binding(N): "name"` doc lines and binding types back out of `shader_bindings.rs`, and found that `wgsl_bindgen` lists layout entries in declaration order, not by index.

### henad-compute, henad-models and henad-app

- `include_shaders!` and the hidden `__shader_support` module sit in henad-compute's `lib.rs`, as the design's [4.6.3] gives them, and henad-compute calls the macro on itself.
  The macro names `include!`, `concat!` and `env!` through `::core`, and both allow lists carry `single_use_lifetimes` beside the design's list.
  `shader_bindings` is therefore private to each crate.
- henad-compute's build script names seven entry points under `src/gpu`: the five primitives, `tests/parity.wgsl`, and the new `grid_dims.wgsl`, a never-dispatched pass whose only job is to reference `henad::dims::Dims`.
  `grid_engine.rs`'s `Dims` is a `pub(crate)` alias of the generated struct, and henad-models' layout assertion against it is gone.
- henad-models' build script is `ShaderBuild::discover("src")?.generate()?`, in a `shader_build` region the docs include, and `ENTRY_POINTS` with its `entry_points` region is gone.
- henad-app's build script runs `discover("src/ui")` beside its commit stamp, which M7 moves.
  Its `agents.wgsl` and `edges.wgsl` now have binding declarations too, unused.
- `scripts/check_packaging.sh` lost its exemption for henad-models' climbing path, and `deny.toml` ties the fxhash advisory to henad-build's pin.

### The gate

**Shader check.**
A worktree of 773a7a5 and the M5 tree were each built `--release` into a scratch target directory.
A script extracted every `SHADER_STRING` from the three crates' `OUT_DIR/shader_bindings.rs`, decoded each `X_naga_oil_mod_X<base32>X` suffix, mapped `shared::prelude` to `henad::dispatch` and `shared::<m>` to `henad::<m>`, and encoded it again.
All 24 shipped entry points (5 primitives, 17 model shaders, `agents` and `edges`) matched byte for byte.
The differences were the expected ones: `grid_dims` is new, `shared::parity` became `tests::parity`, and the old `shared::prelude`, `shared::dims` and `shared::space` entry points are gone.
The two parity shaders differ only in the `CODES` array and the order of two imported constants.

**HENAD_DUMP_WGSL.**
Both release CLIs ran each GPU model for three steps with `--export-stats` and a dump directory.
Each wrote the same 23 files, and with the same name mapping every file matched byte for byte.
The raw dumps therefore differ only inside the mangled import names.

**Codes.** `shader_bindings::henad::space` holds all five codes the parity test reads, and the parity test passes on the Metal device.

**Verified packaging.** `cargo package --workspace --exclude henad-tutorial --locked --allow-dirty` packaged and built all seven crates from their tarballs, henad-build included (16 files, 29.0 KiB compressed).
The first two runs failed to verify henad-build with E0432 on `primitives::wgsl`.
Cargo had kept its extraction of the overlay registry's henad-core 0.2.0 from M1's run, at `~/.cargo/registry/src/-706874a096166f48/`, and never extracted the new tarball over it.
Removing that copy was not enough: the new extraction held `wgsl/`, and the verify failed the same way.
The likely cause is the henad-core the earlier verify had compiled into `target/`, since Cargo never rebuilds a registry crate on a source change (inferred, not traced).
A run with a fresh `CARGO_TARGET_DIR` passed.

### The cargo-level checks

The `downstream` job's steps 7, 13 and 15 [5.4] ran by hand on a scratch crate in the session's scratch directory, outside any git work tree.
The crate depended on `henad-core`, `henad-compute` and `henad-build` at `=0.2.0`, patched through `[patch.crates-io]` to the unpacked tarballs, and `cargo metadata --locked` showed all three with a null source.
It held a CPU model and a GPU model copied from Game of Life, a `src/bin/my-model-cli.rs`, `ShaderBuild::discover("src")` and `include_shaders!()`, under `unsafe_code`, `unreachable_pub` and `unused_qualifications` denied.

- **Step 7.** Two `cargo build -v --locked` runs in a row: the first built with no warning, and the second reported every unit Fresh, with no Dirty line, no Compiling line and no build script run.
  A comment appended to `src/vote.rs` then made `vote` alone Dirty and reran its build script, which rewrote nothing: the generated files kept the first build's time.
- **Step 13.** A new `src/tally/count.wgsl`, referenced by no code, compiled under `cargo clippy --all-targets -- -D warnings` and under `RUSTFLAGS="-D warnings"`, and `shader_bindings::tally::count` and `TALLY_COUNT` appeared.
- **Step 15.** With the GPU model, its insert line and the extra shader deleted, the crate built clean with `ShaderBuild` and `include_shaders!` still in place, and both live `OUT_DIR`s held an empty `shader_bindings.rs` and a `bindings` module with no constants.
  With the `ShaderBuild` line and `include_shaders!` deleted as well, it built and ran.
  The sweep, resume and `BuildChanged` half of step 15 waits for M7's stamps.

After the M5 review, the three steps ran again on a fresh scratch crate against tarballs packaged from the reviewed tree.
The build script printed `cargo:rerun-if-changed=<root>/src` alone, the second build was Fresh throughout, the unreferenced shader compiled under clippy with `-D warnings`, and the CPU-only crate built with an empty `shader_bindings.rs` and no stamp.
With the GPU model and the shader put back, the next build regenerated the full bindings and the CLI listed both models.

### Docs

- `authoring/shaders.md` shows the build script and `include_shaders!` by include, states the naming rules, the reserved root, the import-path rule and the one-line binding form, and corrects line 28: an imported constant reaches the bindings exactly when an entry point references it.
  A note says to deny `unsafe_code` rather than forbid it.
- The GPU Game of Life page lost its "Add our three entries" step and the `entry_points` include, and shows the build script instead.
  The GPU ants page lost its four-entry list, and both pages import `henad::`.
- `reference/primitives.md`, `authoring/gpu-grid-models.md`, `authoring/gpu-agent-models.md` and `developing/gpu-backend.md` name the new paths.
- `developing/architecture.md` and the home page count seven crates, and the graph draws henad-build as a build dependency.
- AGENTS.md has henad-build in its diagram with a crate bullet, seven crates, the rule with henad-build in it, the new home of the shared WGSL, `grid_dims.wgsl` and `CODES`, the three meanings of `primitives`, and the generated-bindings paragraph rewritten for henad-build and `include_shaders!`.

### The M5 review

The maintainer reviewed M5 before its commit and reproduced ten findings on a scratch crate.

1. A crate whose shaders all went kept its stamp, and their return matched it and left `shader_bindings.rs` empty. The empty branch now removes the stamp, pinned by `shaders_removed_and_restored_are_generated_again`, which fails with the removal disabled.
2. A first path component of `wgpu`, `bytemuck`, `std`, `core`, `alloc`, `_root`, `ShaderEntry`, `layout_asserts` or `bytemuck_impls` is refused as `ReservedName`, which now carries the name.
3. `henad` is compared with `eq_ignore_ascii_case` for the root, directories and file stems, and for the warning on a `#define_import_path` line.
4. A `@binding(` line in a module fails the build.
5. `ShaderBuildError`'s `Debug` delegates to `Display`.
6. The rerun line and the warning use the single-colon form.
7. `single_use_lifetimes` joins both allow lists, `output.rs`'s own source joins the stamp, and `scripts/check_packaging.sh` ties `WGSL_BINDGEN_VERSION` to the pin. A changed constant fails the script, as a run with `0.23.4` showed.
8. `open_run` reports the lookup message without a full stop, as every other slot does.
9. `include_shaders!` and the generated assertions name their macros through `::core`.
10. The AGENTS.md lines over 100 columns are rewrapped, and [5.5] says why the verified packaging needs a fresh `CARGO_TARGET_DIR`. `docs/developing/releasing.md` does not exist yet.

### Edited tree

```text
.
├── AGENTS.md                            ~ M4 review messages, henad-build, the shared WGSL, generated bindings
├── CHANGELOG.md                         ~ Unreleased: runnable, the reworded message, M5's Added, Changed, Removed
├── Cargo.toml                           ~ henad-build and heck, wgsl_bindgen pinned at =0.23.3
├── Cargo.lock                           ~ henad-build
├── deny.toml                            ~ the fxhash reason
├── zensical.toml                        ~ nav entry #34
├── scripts/check_packaging.sh           ~ no climbing exemption, the wgsl_bindgen pin check
├── dev-docs/48-library/design-v6.md     ~ [4.4.6] wording, [5.5] fresh CARGO_TARGET_DIR
├── crates/
│   ├── henad-core/src/
│   │   ├── explore/fingerprint.rs       ~ const fn writes
│   │   └── authoring/
│   │       ├── model/gpu_agent_model.rs ~ henad:: imports in the docs
│   │       └── primitives/
│   │           ├── mod.rs               ~ pub mod wgsl
│   │           └── wgsl/                + mod.rs, and dispatch, dims, rng, space, reduce_tree (moved)
│   ├── henad-build/                     + Cargo.toml, README.md, licences
│   │   └── src/
│   │       ├── lib.rs                   + ShaderBuild, ShaderBuildError
│   │       ├── paths.rs                 + the walk and the name checks
│   │       ├── binding_lines.rs         + the binding reader
│   │       ├── output.rs                + the generated files and the stamp
│   │       └── tests/                   + mod.rs, support.rs, paths.rs, binding_lines.rs, generate.rs
│   ├── henad-compute/
│   │   ├── Cargo.toml, build.rs         ~ henad-build, ShaderBuild::new
│   │   └── src/
│   │       ├── lib.rs                   ~ include_shaders!, __shader_support
│   │       ├── entry/set.rs             ~ runnable, the NeedsGpu message
│   │       └── gpu/
│   │           ├── grid_dims.wgsl       + Dims into the bindings
│   │           ├── grid_engine.rs       ~ Dims a generated alias
│   │           ├── shared/              - moved to henad-core and tests/
│   │           ├── primitives/*.wgsl    ~ henad::dispatch
│   │           ├── primitives/dispatch.rs   ~ WORKGROUP from henad::dispatch
│   │           └── tests/parity.{wgsl,rs}   ~ moved, CODES, henad:: paths
│   ├── henad-models/
│   │   ├── Cargo.toml, build.rs         ~ henad-build, discover
│   │   └── src/
│   │       ├── lib.rs                   ~ include_shaders!, no Dims assertion
│   │       ├── gpu_*/*.wgsl             ~ henad:: imports
│   │       └── tests/tutorial/gpu_foraging.rs   ~ henad::rng
│   ├── henad-app/
│   │   ├── Cargo.toml, build.rs         ~ henad-build, discover
│   │   └── src/
│   │       ├── lib.rs                   ~ include_shaders!
│   │       ├── init.rs                  ~ "Returns" docs
│   │       ├── state.rs                 ~ runnable, select_model, lookup_message, the test
│   │       └── ui/model.rs              ~ NO_MODEL_RUNS
│   └── henad-cli/src/main.rs            ~ runnable, the crate doc rewrapped
└── docs/
    ├── index.md                         ~ seven crates
    ├── authoring/{shaders,gpu-grid-models,gpu-agent-models}.md   ~
    ├── developing/{architecture,gpu-backend}.md                  ~
    ├── developing/agent-record/20261001-34-library-shaders.md    +
    ├── guide/first-model/{gpu-game-of-life,gpu-ants}.md          ~
    └── reference/primitives.md          ~
```

### Hot paths

M5 changes no kernel and no tick.
The compiled shaders are byte-identical once the import names are mapped, and `Dims` has the layout the hand-written struct had: `[u32; 2]` twice, now with the generated `align(8)`.
The build adds a directory walk and a hash to each rerun of a crate's build script.

## State after

M5 is implemented and uncommitted on `48-library`, on top of cdaa134.
The workspace version stays 0.2.0.

- `HENAD_REQUIRE_GPU=1 ./check.sh` passes: 991 tests, none failed, with the packaging, cargo-deny and docs steps and the web build.
  That is M4's 975, plus henad-build's thirteen tests and its doc test, and the two tests of `wgsl/mod.rs`.
  Before the M5 review it passed at 988. The first run after the review stopped at clippy, on two assertions in the new tests without a closing `;`.
- `uv run --locked zensical build` passes with its path checks.
- The shader check and the dump comparison match every shipped shader, and the codes are in the bindings.
- `cargo package --workspace --exclude henad-tutorial --locked --allow-dirty`, in a fresh `CARGO_TARGET_DIR`, verifies all seven crates from their tarballs, before the M5 review and after it.
- Steps 7, 13 and 15 of the `downstream` job pass by hand on a scratch crate, before the M5 review and after it.
- A release app with `--features inspection`, driven through the egui MCP server, switched from SIR Epidemic to Boids Flocking (GPU) with that model's parameters loaded, and Build built it with the GPU pacing controls and no fault. The app's saved state was backed up before and restored after.

## Issues found & future directions

- **A stale overlay extraction.** `cargo package --workspace` extracts its overlay registry under `~/.cargo/registry/src/` at a path that depends on the target directory alone, and keeps that extraction across runs of one version. A verify of a later tree then compiles the earlier tarball's henad-core, and the compiled copy in `target/` survives deleting the extraction. The design's release checklist [5.5] now runs the hand-run verification in a fresh `CARGO_TARGET_DIR`, and `docs/developing/releasing.md` takes the line when it is written.
- **A sampled texture is refused.** The reader knows the three `BindingKind`s, and a downstream render shader with a `texture_2d` or a `sampler` under the shader root fails the build. henad-compute's own `view/display.wgsl` stays outside its entry list for that reason. A render layer of a model's own would need either another root or a fourth kind.
- **The app's binding declarations are unused.** `binding_decls` for `agents.wgsl` and `edges.wgsl` exist because every crate now gets them, and `dead_code` covers them.
- **Next.** M6 (the programmatic API) depends on M3, and M7 (the stamps) on M3 and M5, and puts `stamp_commit` into henad-build.

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
