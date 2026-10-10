---
date: 2026-10-10
title: "Runtime grid rules and the model file"
description: "Milestone M4 of #49. A grid rule is a GPU grid model declared as data, with a WGSL step composed while the program runs after an engine template, checked at the line of each problem, and run on the GPU grid engine. A model file holds a rule as one TOML file, and its canonical text is the rule's code identity. The Life rule steps through the cells of the example Game of Life bit for bit and passes the testing kit."
icon: material/grid
status: ai-generated
model: claude-opus-5-5 (Claude Code)
issue: "#49"
state: "opening gate passed, grid rules, model files, the facade feature and the tests in place, reviewed and fixed, check.sh and check_hot.sh passing with HENAD_REQUIRE_GPU=1 after the review fixes, uncommitted on top of f2d7760"
baseline_commit: f2d7760
delta_state: "uncommitted on `49/m4-runtime-grid-rules`, 12 new files, 32 changed files and this record"
---

# Runtime grid rules and the model file

> Milestone M4 of #49 (dynamic model loading), decisions 3, 4, 9 and 11 of its plan.
> A user writes the step of a GPU grid model in WGSL, and a running program composes it, checks it and runs it, with no build.
> henad-compute gains, behind a new `runtime-wgsl` feature, `compose_wgsl`, the one function over naga_oil 0.23, and `gpu::runtime_grid` with `GridRule`, `RuleState` and `CompiledGridRule`.
> A rule's text defines `step_cell(cell, rng)`, and an engine template in front of it declares `Params`, `neighbor`, `moore_count`, `von_neumann_count` and a constant per state.
> The checks refuse loops, bindings, entry points, atomics, barriers, shared writes, engine buffers other than `params`, built-in names at module scope, template names, variables past WGSL's 8192-byte limits and a step past 65536 expressions and statements, each at its line and character column in the rule's text.
> henad-explore gains, behind `model-files`, `ModelFile`: a rule as one TOML file, with a canonical text that is the rule's code identity.
> henad-core gains `CodeDiagnostic`, and the facade a `model-files` feature with `henad::rules`.
> The opening gate passed: runtime composition equals every stock `SHADER_STRING` byte for byte, a corpus of 3600 edits never panics the composer, and the release web build grows by 1.43 MB raw and 0.50 MB gzipped.
> `GridRule::life()` steps through the cells of `game_of_life` bit for bit for 64 ticks on a 128 by 128 grid, and passes every check of the testing kit.
> Three reviews followed, with seventeen distinct findings, and the session fixed sixteen: among them, a rule could read `state_out` and `tick_out`, which other cells write in the same pass, and a chain of helpers with no loop could run 2^60 calls a cell.
> Arrays are now held to WGSL's byte limits, a model file places declaration problems at their tables, and a `"""` step holding `'''` is refused.

## State before

`49/m4-runtime-grid-rules` branched from `49-dynamic-loading` at `f2d7760`, which holds rounds 1 and 2 as placeholder commits: D1, M1 (#50), M3 (#52), D2 and M2 (#51).
M3 left the GPU grid engine non-generic, built from the crate-private `GpuGridSpec` with its functions behind the `GridHooks` trait, and M2 the crate-private `EntryDeclarations` and `ModelEntry::new_gpu`, `ModelCode` and `ModelSource::runtime`.
`dev-docs/49-dynamic-loading/plan.md` governs, with its grid rule template and model file sections.

naga_oil reached no runtime graph: wgsl_bindgen composed every shader at build time inside henad-build.
Nothing in Henad composed or checked WGSL while a program ran, and no type described a problem in a piece of code.

## What was done

### Opening gate

The plan's three checks ran before anything else was built on the composer.

**(a) Parity.**
`compose_wgsl` adds the shared modules in `SHARED_WGSL_MODULES` order, then the caller's `#define_import_path` modules, composes with `Composer::default()` and `make_naga_module`, validates with `ValidationFlags::all()` and `Capabilities::all()`, and writes with `naga::back::wgsl::write_string(.., WriterFlags::empty())`, as wgsl_bindgen 0.23.3's `EmbedSource` does.
Every compute shader composes to the text henad-build embedded, byte for byte.
wgsl_bindgen writes the text into a raw string literal that opens with a line break (`create_shader_raw_string_literal`), so `SHADER_STRING` equals `"\n"` followed by the composed text, and the tests compare exactly that.

- `runtime_composition_matches_every_stock_shader` (henad-models, `src/tests/runtime_parity.rs`): the 17 compute entries of `gpu_ants`, `gpu_boids`, `gpu_game_of_life` and `gpu_sir`, `gpu_ants`' with `gpu_ants/state.wgsl` as a module.
- `runtime_composition_matches_build_time_shader_strings` (henad-compute, `src/gpu/tests/compose.rs`): henad-compute's seven entries, `tests/parity.wgsl` and `grid_dims.wgsl` among them.
  henad-compute's `shader_bindings` are crate-private, and henad-models cannot reach them, so these sit in henad-compute.

**(b) Panics.**
`compose_never_panics_on_the_mutation_corpus` (henad-models) makes 200 seeded edits of each of the 17 entries and of `gpu_ants/state.wgsl`, a third of them with a second edit on top: deleted characters, a repeated line, two lines swapped, one of 42 junk lines (malformed imports, naga_oil's other directives, WGSL directives, `virtual` and `override`, `@binding(auto)`, decorated names, `::` paths, stray comment markers), or a junk character.
Each of the 3600 cases runs under `catch_unwind`, and none panics or reaches the panic backstop inside `compose_wgsl`.
The run takes about 10 seconds in a test build.

A one-off experiment, deleted afterwards, ran the same corpus against naga_oil with no check in front.
None of the 3600 panicked.
A targeted list did find a panic: `override fn pcg_hashX_naga_oil_mod_X1X(x: u32) -> u32 { return x; }` makes naga_oil decode `1` as base32 and unwrap the error (`DecodeError { position: 0, kind: Length }`).
The same list showed naga_oil accepting, with no error, several things the user never meant: `#import "x.wgsl"::item`, `#define_import_path` in an entry, `virtual fn`, `@binding(auto)` (renumbered), and `henad::rng::pcg_hash(..)` called by its path with no import.
So a strict directive check sits in front of naga_oil, as the plan's fallback asks, and the corpus carries the panicking line among its junk lines.
The call into naga_oil also runs inside `fault::catching`, a backstop on native only, since the threaded web build aborts on a panic.

**(c) Size.**
The web app was built with `./scripts/build_web.sh`, before any change and with a temporary call to `compose_wgsl` in `start_web` (taking the window's name as its text, so nothing folds it away), reverted afterwards.
Without a call, the linker drops naga_oil and the feature costs nothing.

| Build | Before | After | Delta |
|---|---|---|---|
| `build --release` (wasm-opt `-O2`), raw | 20,514,378 | 21,943,521 | +1,429,143 (+7.0%) |
| `build --release`, gzip -9 | 8,600,511 | 9,103,137 | +502,626 (+5.8%) |
| `build` (debug, as `check.sh`), raw | 47,219,359 | 50,191,390 | +2,972,031 |
| `build` (debug), gzip -9 | 12,151,424 | 12,907,052 | +755,628 |

The release delta sits under the 1.5 MB raw that `design-synthesis.md` named as the line for going back to the maintainer.
naga's WGSL front end and validator were already linked on wasm32 through eframe and wgpu's `webgl` feature.
The new code is naga_oil, its regex, tracing and codespan dependencies, and naga's `wgsl-out` writer.

### henad-core: `CodeDiagnostic`

`diagnostic.rs` holds `CodeDiagnostic`, `#[non_exhaustive]` with public fields: severity, message, file, byte span, line and column (both counted from 1, the column in characters) and notes.
`error`, `with_file`, `with_span`, `with_note` and `at(text, span)` build one, the last reading the line and column from a text with `\n`, `\r\n` or a lone `\r` as line ends.
`DiagnosticSeverity` is `#[non_exhaustive]` with the one variant `Error`, since nothing yet produces a warning.
`Display` writes `file:line:column: severity: message` and a line per note, leaving out what the diagnostic lacks.

`helpers::check_choice_options` is the check `choice_param` panics on (a name empty or with spaces at an end, a name that reads as an index, a name twice), returning the reason instead, for option names read at run time.
`choice_param` now panics with `choice 'x': ` and that reason.

### henad-compute: `compose_wgsl` (`gpu/compose.rs`)

`compose_wgsl(entry, modules)` returns a `ComposedWgsl` or a list of `CodeDiagnostic`s, and no naga or naga_oil type crosses it.
The crate-private `compose(entry, modules, base)` returns a `Composition` (the naga module, its `ModuleInfo`, and what it takes to place a span), and `base` is the byte offset where the caller's own text starts in the entry, which a grid rule sets to its template's length.

The directive check reads every file before naga_oil:

- an `#import` line names a known module and one or more items on one line, as `#import <module>::{a, b as c}` or `#import <module>::item`, with an optional `;`;
- a module's `#define_import_path` names a module path, and an entry's is refused;
- any other `#` line is refused, and so are a `#`, `::` or `"` elsewhere in code, a line that opens with `enable`, `requires` or `diagnostic`, the words `virtual` and `override`, `@binding(auto)`, naga_oil's decoration anywhere in the text, files over 1 MiB and more than 1024 modules.

It blanks comments the way naga_oil does, with nested block comments.

A naga span in the composed module is not a place in the file, and the mapping takes three steps:

1. The module index sits above bit 21 of a span, a private constant of naga_oil (`SPAN_SHIFT`), which `compose.rs` copies and masks.
2. naga_oil parses each file behind an import header whose length it keeps to itself.
   For a parse or validation error, its `ErrSource` holds the offset and the preprocessed text.
   For a module that composed, `Composition::entry_start_offset` reads the offset back from an anchor function of the entry whose line naga_oil leaves alone, here the template's `neighbor`, and recomputes the preprocessed text through naga_oil's public `Preprocessor`.
3. naga_oil keeps every line but writes each imported name decorated, as `next_bitsX_naga_oil_mod_XNBSW4YLEHI5HE3THX`, which moves every column after it.
   `map_column` aligns the tokens of the line naga read with the line as written, through naga_oil's public `Tokenizer`, and a column inside a decorated name is clamped to the name.

A label that lands in front of `base` gives no place, and a note says the problem lies in the engine's template.
A parse error takes its place from its first label in the caller's text.
A validation error lists its labels from the function to the expression at fault, and takes its place from the last, so `let wide = i64(cell);` reports at `i64`, not at the line of `fn step_cell`.
Messages are undecorated before they are returned.

### henad-compute: grid rules (`gpu/runtime_grid/`)

**`GridRule`** (`mod.rs`) is `#[non_exhaustive]` with public fields: id, name, description, its own parameters, its states (`RuleState`: label, RGBA colour, optional `initial` f32 parameter id), `Boundary`, default width and height, and the step text.
`GridRule::new(id, step)` and `GridRule::life()` build one, the second from `LIFE_STEP` (`life_step.wgsl`, outside `ENTRY_POINTS`) with Dead, then Alive at the share `density` 0.3 on a 256 by 256 torus.
`param_descriptors` puts `grid_width` and `grid_height` (1 to 16384) in front of the rule's own, every one applying on reload.
`GridRule::CODE_FORMAT` is `henad-grid-rule/1`, the format of a rule's `ModelCode`, whose version goes up with any change to what a rule's text does.

**The template** (`template.rs`) comes first in the composed entry, and the rule's text follows it, so a line of the text keeps its number after the template's lines:

```wgsl
// Engine template of a grid rule. The rule's own text follows it.
#import henad::rng::{pcg_hash as henad_pcg_hash}
#import henad::space::{offset_cell as henad_offset_cell, TORUS as HENAD_TORUS, BOUNDED as HENAD_BOUNDED}

struct Params {
    width: u32,
    height: u32,
    seed: u32,
    density: f32,
}

@group(0) @binding(0) var<storage, read> state_in: array<u32>;
@group(0) @binding(1) var<storage, read_write> state_out: array<u32>;
@group(0) @binding(2) var<storage, read> tick_in: array<u32>;
@group(0) @binding(3) var<storage, read_write> tick_out: array<u32>;
@group(0) @binding(4) var<uniform> params: Params;

const NUM_STATES: u32 = 2u;
const DEAD: u32 = 0u;
const ALIVE: u32 = 1u;
```

It goes on with `var<private> henad_cell`, `neighbor(dx, dy)` (state 0 past a bounded edge), `moore_count(state)`, `von_neumann_count(state)`, and `main`, which advances the one-word tick, seeds `rng` from `pcg_hash(seed ^ pcg_hash(index ^ pcg_hash(tick)))` and writes `min(step_cell(cell, &rng), NUM_STATES - 1u)`.
The template's own names start with `henad_` or `HENAD_`.
Each parameter takes a 4-byte field named by its id, `f32` for an F32 and `u32` for a U32, a bool (0 or 1) or a choice's index.
The display shader writes a palette constant, and the reduce shader counts the cells in each state into one counter per state, one stat per state.

**The checks** (`checks.rs`) run in three passes and stop after the first that finds a problem:

- *Declarations*, public as `GridRule::check_declarations`, each problem with the `RuleDeclaration` it is about (`Id`, `Grid`, `State(i)`, `Param(i)`, or none for a count): the id grammar, the grid sides, at most 256 states and 64 parameters, parameter ids in the id grammar and outside WGSL's reserved words (`naga::keywords::wgsl::RESERVED`), `width`, `height`, `seed`, `tick`, `grid_width`, `grid_height` and `henad_`, a default within finite bounds, a positive finite step, choice options that `check_choice_options` passes, `percent` on an f32 alone, one constant per state (labels to upper snake case, ASCII letters and digits) outside the template's names and WGSL's reserved words (`Null` gives `NULL`), an `initial` that names an f32 parameter, and one state without a share.
- *Text*: at most 64 KiB, no template name or `henad_` name declared again, no WGSL predeclared name (`naga::keywords::wgsl::BUILTIN_IDENTIFIERS`) declared at module scope, no `@group` or `@binding`, no entry point of its own (`@compute`, `@vertex`, `@fragment`), each with blanks or a comment after the `@` as naga allows, and a `fn step_cell`.
  A module-scope `fn min` would take the place of the built-in in the template's `min(step_cell(..), NUM_STATES - 1u)`, and an `alias vec2` would break the template's own lines.
  A local `let max` shadows nothing the template reads, and passes.
- *IR*, after the composition: every function whose span lies in the rule's text, and once each function outside it that a rule calls.
  The walk accepts `if`, `switch`, `return`, `break`, `continue`, stores and calls, and refuses a loop, an atomic, a barrier and any other statement.
  A store must reach a local, a pointer argument into function or private memory, or a `private` variable of the rule's text.
  A function of the text names no engine buffer but `params`: other cells write `state_out` and `tick_out` in the same pass, and a read of `state_in` by index past the grid returns what the backend's bounds policy decides.
  A module-scope variable of the text must be `private` without a binding, and the module holds no entry point but `main`.
  No variable, constant or value holds an atomic.
  The locals of each function take at most 8192 bytes (`MAX_FUNCTION_BYTES`), the shader's `private` variables at most 8192 with the template's `henad_cell` (`MAX_PRIVATE_BYTES`), and a value or constant at most 8192, WGSL's implementation limits, which Tint enforces in a browser and naga does not.
  Sizes come from naga's `TypeInner::try_size`, so a struct counts every member.
  One call of `step_cell` runs at most 65536 expressions and statements (`MAX_STEP_COST`), counting a callee's body once per call.
  `function_costs` sums each function in handle order, since naga puts a callee before its caller, and the problem lands in the deepest function of the text past the limit, at the call that takes it there.
  Life's `step_cell` costs 1196.

A problem in an imported function is reported at the rule's call, as in "this calls `block_sum`, which writes `scratch`, memory the cell does not own".
A problem in the template, such as a `step_cell` with the wrong signature, carries a note with the signature the engine calls.

**`CompiledGridRule`** holds the rule and its three composed shaders.
`entry(code)` fills `EntryDeclarations` (one stat and one palette colour per state, `Structure::GpuGrid` over `state` and `tick`, no actions, exact replay) and builds a `GpuGridSpec` with owned sources, static binding lists and `RuleHooks`, then calls the crate-private `ModelEntry::from_grid_spec` with `ModelSource::runtime(id, code)`.
The factory captures the spec, so it builds on any device it is handed.
A problem in the engine's own display or reduce shader has no file and no place.
`RuleHooks` seeds the cells on the CPU from `grid_init_rng(seed)`, one `next_bits` word per cell in row-major order.
The states with a share take the low words in declaration order, each below the running f32 total of the shares times `u32::MAX`, and the rest state takes the others.
For Life that is `game_of_life::init`'s draw and threshold exactly.
The uniform block holds the width, the height, a word of the seed mixed with its own domain separator, and a word per parameter, padded to a multiple of 16 bytes.

`GridHooks::step_params_bytes` gained the run's seed, which a model type ignores.

### henad-compute: one spec per registered GPU grid model

Record #52 left an open question: `register_gpu_grid_model` boxed `GpuGridState<M>` and built a spec on every `demand` call.
It now reads one `Arc<GpuGridSpec>` from the model and passes it to `ModelEntry::from_grid_spec`, whose factory and demand closure share it and which boxes `GpuGridEngine` itself, as a rule's entry does.
The adapter's forwarders and the engine's methods are the same set and every trait default is untouched, so no behaviour changes.
The GPU sweep, track, parity and kit tests pass as they were, and every GPU grid model and the rule now run through one boxed type.
`gpu::grid_spec` became `pub(crate)` for it, and `entry::check_id_grammar` is the id check `ModelSet` already ran, shared with the rule.

### henad-explore: model files (`model_file.rs`)

Behind the `model-files` feature, which turns on henad-compute's `runtime-wgsl`:

```toml
format = "henad-model"
format_version = 1
kind = "gpu_grid"
id = "life_rule"
name = "Life rule"
description = "Conway's Game of Life, written as a grid rule"

[grid]
width = 256
height = 256
boundary = "torus"

[[state]]
label = "Dead"
color = [21, 21, 21, 255]

[[state]]
label = "Alive"
color = [0, 230, 118, 255]
initial = "density"

[[param]]
id = "density"
label = "Initial Density"
kind = "f32"
min = 0.0
max = 1.0
default = 0.3
step = 0.01

[step]
wgsl = '''
fn step_cell(cell: u32, rng: ptr<function, u32>) -> u32 {
    ...
}
'''
```

- Every table refuses an unknown key, and a TOML error comes back as a `CodeDiagnostic` at its line and column, as in `9:1: error: unknown field `depth`, expected one of `width`, `height`, `boundary``.
- A parameter uses the `--params --json` keys: `id`, `label` (the id when absent), `kind`, `min`, `max`, `default` (an option's name for a choice), `step`, `options` and `format` (`percent`).
  A key that does not fit its kind is refused.
- A colour takes three or four bytes.
  `boundary` is `torus` or `bounded`, `torus` when absent.
- The step is a string the file holds verbatim, so a problem in it maps to a file line by adding the string's start offset, from `toml::Spanned`.
  A string whose text differs from its literal, through escapes, is refused with a request for `'''`, and so is a step holding `'''`, in `parse` as in `from_rule`, since the canonical text could not hold it.
- A choice's options pass `check_choice_options` in `parse`, or the default, written by name, would not read back to its index.
- The header keys are read first, ignoring every other key, so a version 2 file with new keys reads as version 2 before an unknown key is refused.
- Line ends read as `\n`, and `LineEnds` maps every span back to the text as passed, one byte further per `\r\n` before it.

`ModelFile` holds the rule, the text it was read from, and the places of its `id`, `[grid]`, each `[[state]]` and `[[param]]`, and its step.
`parse`, `from_rule` (which refuses a step a literal string cannot hold and a float that is not finite, then reads its canonical text back through `parse`), `from_code` (which checks the format), `rule`, `to_toml`, `code` and `entry` are its API.
`to_toml` writes the canonical text: the keys above in that order, one form per value (floats in Rust's shortest `Debug` form, colours with their alpha, strings as basic strings), and the step verbatim.
`code` is `ModelCode::new(GridRule::CODE_FORMAT, to_toml())`, so comments outside the step, key order, blank lines and number forms leave the code alone.
`entry` runs `GridRule::check_declarations` and places each problem at its table or key, then compiles the rule and places each problem of the step in the file.
`ModelFileError` is `Toml`, `Format` or `Rule`, with `diagnostics()`.

`specs/models/life_rule.toml` is the example, with comments, and reads to the canonical text of `GridRule::life()`.
The spec tests read the top level of `specs/` alone and leave it out.

### Facade, hosts and CI

- The facade has a `model-files` feature with `henad::rules` (`GridRule`, `RuleState`, `RuleDeclaration`, `CompiledGridRule`, `LIFE_STEP`, the `MAX_*` limits, `ModelFile`, `ModelFileError`, `FORMAT`, `FORMAT_VERSION`, `KIND_GPU_GRID`, `compose_wgsl`, `WgslFile`, `ComposedWgsl`), `CodeDiagnostic` and `DiagnosticSeverity` at its root, and `check_choice_options` in `henad::authoring`.
  `tests/facade_paths.rs` names them in `named_only`, in functions no test calls.
  The README's feature table and docs.rs features name it.
- henad-app and henad-cli take henad-explore with `model-files`.
  Neither calls it yet, so their binaries leave the composer out.
- CI's `features` job checks the facade with `model-files` alone, and the `package` and `msrv` jobs add it to their feature lists, as does the release checklist.

### hot-host's lockfile

`examples/hot-host/Cargo.lock` gained `naga` and `naga_oil` in henad-compute's dependency list, written offline by `cargo metadata`.
Every package they reach was already in that lock through wgsl_bindgen, so no version moved, and `check_packaging.sh` finds the shared packages equal to the root lock's.
Without it, `scripts/check_hot.sh` stops at its `--locked` `cargo tree`.

### Licences

naga_oil, data-encoding, termcolor and codespan-reporting 0.12 were already in the graph through wgsl_bindgen, a build dependency, and cargo-about lists build dependencies.
`cargo about generate about.hbs` (0.9.1) writes the committed `docs/license.html` byte for byte, and `cargo deny --locked check` passes with `deny.toml` unchanged.

### Docs

- `docs/reference/api.md`: the crate table, the `model-files` feature, the two root types and `henad::rules`.
- `docs/reference/primitives.md`: a grid rule refuses loops and walks no table, and reads its neighbours through `neighbor`, `moore_count` and `von_neumann_count`.
- `docs/authoring/shaders.md`: a grid rule's step imports the shared modules the same way, composed while the program runs.
- `AGENTS.md`: the crate list, the dev-dependency edges, `CodeDiagnostic` and `check_choice_options`, `compose.rs` and `runtime_grid/` under henad-compute, the monomorphisation sentence and `from_grid_spec`, `model_file.rs` under henad-explore, `henad::rules` in the facade, the packaging lines, and a GPU-traps entry on span mapping.
- `CHANGELOG.md`: five Added lines and two Changed lines under Unreleased.

The user guide and the model file reference come with M6 and M7.

### Tests

| Test | Where | What it holds |
|---|---|---|
| `runtime_composition_matches_build_time_shader_strings` | henad-compute `gpu/tests/compose.rs` | henad-compute's seven entries compose to their `SHADER_STRING` |
| `runtime_composition_matches_every_stock_shader` | henad-models `tests/runtime_parity.rs` | The 17 example entries compose to theirs, `gpu_ants`' with its module |
| `compose_never_panics_on_the_mutation_corpus` | same | 3600 edits never panic, and never reach naga_oil's panic |
| `naga_oil_span_encoding_is_unchanged` | henad-compute `gpu/tests/runtime_grid.rs` | Imported functions carry the indices 3 (`henad::rng`) and 4 (`henad::space`) above bit 21, the entry 0. A helper and `step_cell` after lines with decorated names and a multi-byte constant map to their bytes as written |
| `a_syntax_error_points_at_the_user_line_and_column` | same | An error after an imported name and two multi-byte characters, below a comment of accented text, sits at its line, its character column and its byte, with no file. A validation error sits at its expression, with no signature note |
| `an_error_inside_the_template_has_no_user_line` | same | A `step_cell` of one argument gives an error with no place and the signature as a note |
| `a_loop_in_user_code_is_refused_at_its_line` | same | `loop` in a helper, `for` and `while` in `step_cell`, each at its line and column |
| `bindings_and_foreign_directives_in_user_text_are_refused` | same | A binding (once per declaration), a template name, `#ifdef`, `#define_import_path`, `enable`, a `::` path, a binding split by a comment, an entry point of the rule's own, also as `@ /* */ compute`, a module-scope `fn min` and `alias vec2`; a local `let max` and an `#import` pass |
| `shared_writes_and_barriers_are_refused` | same | Writes to `state_out`, `tick_out` and `henad_cell`, one problem per engine-buffer write, reads of `state_out`, `tick_out`, `tick_in` and `state_in` by index, a workgroup variable, a barrier, an atomic, a call to `block_sum`, a write through a storage pointer argument; a write to the text's own private variable and a read of `params` pass |
| `oversized_arrays_and_text_are_refused` | same | 8192 bytes of locals and a 512-entry table pass; 8196 bytes of `private` variables with `henad_cell`, a local struct of three 1024-word arrays, a 64 by 64 nested array and two locals of 8196 bytes are refused at their lines, and so is a text over 64 KiB |
| `a_call_chain_past_the_work_limit_is_refused` | same | Sixty helpers, each calling the one before twice, are refused once, at the first level past 65536, and Life's step costs under a 32nd of the limit |
| `a_param_named_seed_is_refused` | same | `seed`, `width`, `tick`, `grid_width`, `loop`, `henad_rate` and `Rate` |
| `a_parameter_its_kind_cannot_hold_is_refused` | same | A reversed f32 range, a step of 0, a u32 default past its bounds, an option `1`, an option twice, no options, `percent` on a u32, each at its `RuleDeclaration::Param` |
| `two_states_with_one_constant_name_are_refused` | same | `Dead` and `dead!`, a state named `Num states`, and one named `Null` at `RuleDeclaration::State(0)` |
| `the_params_block_matches_the_naga_layout` | same | naga's `Layouter` puts the fields at 0, 4, 8 and on, 28 bytes for four parameters, and the engine's 32 bytes hold each value in its field |
| `a_grid_rule_binds_at_most_five_storage_buffers` | same | The entry's needs and every pass stay at 5 or below, and the static binding lists follow the composed shaders' bindings in order |
| `a_result_past_the_last_state_is_clamped` | same, GPU | A step returning 7 in a three-state rule leaves every cell in the last state |
| `a_random_rule_draws_a_fresh_stream_each_tick` | same, GPU | A rule that draws every cell afresh, over six ticks: each Alive count equals the stream recomputed on the CPU from the uniform's seed word, and no two ticks in a row agree |
| `the_life_starter_matches_game_of_life` | henad-models, GPU | 128 by 128, seed 11, 64 ticks: the Alive and Dead counts every tick and every cell every 8 ticks equal the CPU model's, the cells read back by drawing the display texture |
| `a_grid_rule_conforms` | same | `assert_set_conforms` on a baseline device over the Life starter and that random rule, ThreadCount skipped as for every GPU model; the random rule's `FullSubmission` compares one 64-step submission with single steps |
| `a_model_file_round_trips_through_to_toml` | henad-explore `tests/model_file.rs` | A rule with every parameter kind, a percent format, a third state, a bounded grid and accented text round-trips, through `from_code` too; a `"""` step holding `'''` is refused at its line, and a repeated option at its table |
| `unknown_keys_are_refused` | same | An unknown key at the top and in each table at its line, version 2 and another kind refused, and version 2 with a new table read as version 2 |
| `a_declaration_error_points_at_its_table` | same | A bad id at its key, a grid width of 0 at `[grid]`, two states with one constant at the second's `[[state]]`, a parameter named `seed` at its `[[param]]` |
| `the_canonical_code_ignores_formatting` | same | A CRLF file with comments, another key order, a 3-byte colour, `0.30`, `1e-2` and `max = 1` has the starter's code; a comment in the step or another width changes it |
| `a_wgsl_error_points_at_its_file_line` | same | An error in the step at its file line, character column and byte, also with the text on the line of `'''`, and in a CRLF file at its byte as written |
| `every_example_model_file_loads` | same | Each file under `specs/models` builds a runtime entry, and `life_rule.toml` reads to the starter |

`CodeDiagnostic` has two unit tests, the composer five (comment blanking, column mapping, undecoration, character columns, the directive check's lines), and the template one (constant names).
henad-core's three `choice_param` panic tests read the new messages.

### Review findings

Three reviews read the first version of this milestone, with 21 findings between them, 17 of them distinct.
The session fixed:

- a rule that named `state_out`, `tick_out`, `tick_in` or `state_in` (all three reviews' first finding in one form or another): reading `state_out` or `tick_out` raced the other cells while the entry declared exact replay, and `state_in[cell * 99999999u]` read what the backend's bounds policy gave.
  A function of the rule now names no engine buffer but `params`;
- a chain of helpers, each calling the one before twice, which held no loop and ran 2^60 calls a cell (the second review's blocker, confirmed in a scratch crate).
  `MAX_STEP_COST` now bounds one call of `step_cell`;
- an array limit that counted elements, took the largest struct member and checked each local alone, so a 2 KiB table was refused while megabytes of nested structs passed.
  Variables are now held to WGSL's 8192-byte limits per function and for the shader's `private` variables;
- `ModelFile::parse`, which read a `"""` step holding `'''` that its own canonical text could not write back.
  `check_step` now runs in `parse` and `from_rule` alike;
- parameter declarations the rest of Henad refuses: repeated, blank or numeric option names, defaults outside their bounds, a step of 0, `percent` off an f32.
  They are refused in the rule's checks, and repeated options also in `parse`, through henad-core's new `check_choice_options`;
- a declaration problem with no line in a model file.
  `RuleDeclaration` and `GridRule::check_declarations` let `ModelFile::entry` place each at its table or key;
- a state labelled `Null`, whose constant `NULL` is a reserved word and broke the template with no line;
- a module-scope `fn min` or `alias vec2`, which replaced the built-in in the template's own lines, defeating the clamp to the last state, or broke the template under a note that blamed it;
- an entry point written `@ compute`, which the text check missed.
  The text check now reads past blanks and comments after `@`, and the module check refuses any entry point but `main`;
- a validation error placed at the first line of its function, now at the expression at fault, and the signature note added to every unplaced problem, now only to a problem in the template;
- diagnostics from the engine's display and reduce shaders, which kept a file and a span that `ModelFile::entry` would have moved into the step;
- spans of a CRLF model file, which pointed into its LF form.
  `LineEnds` maps them back to the text as passed;
- a version 2 file with new keys, which reported an unknown key, not its version.
  The header keys are now read first;
- no test that drew from the rule's random stream.
  `a_random_rule_draws_a_fresh_stream_each_tick` recomputes it on the CPU, and the kit test checks a random rule beside Life;
- `WgslFile`'s derived `Debug`, which printed the whole shader, and two `Debug` impls that ended in `finish()`;
- the GPU grid entry built in two places with the same code, now `ModelEntry::from_grid_spec`;
- `DiagnosticSeverity::Warning` and `Note`, `CodeDiagnostic::warning` and `is_error`, which nothing produced or called;
- AGENTS.md's claim that `named_only` reads the Life starter back, when no test calls it.
  The dead asserts are gone.

One finding is left to the maintainer: henad-app and henad-cli turn on `model-files` unconditionally, so a web app on the template's `app` feature will carry naga_oil once M6 calls it.
The brief asked for exactly that, and the open questions below carry the review's alternative.

### Edited tree

```text
.
├── AGENTS.md, CHANGELOG.md, zensical.toml           ~ runtime WGSL, grid rules, model files, nav #53
├── Cargo.toml, Cargo.lock                           ~ naga and naga_oil workspace entries
├── examples/hot-host/Cargo.lock                     ~ henad-compute's two optional dependencies
├── check.sh                                         ~ comment: the facade's six features
├── .github/workflows/ci.yml                         ~ model-files in the features, package and msrv jobs
├── crates/
│   ├── henad-core/src/
│   │   ├── diagnostic.rs                            + CodeDiagnostic, DiagnosticSeverity
│   │   ├── helpers.rs                               ~ check_choice_options, choice_param over it
│   │   └── lib.rs                                   ~ mod diagnostic
│   ├── henad-compute/
│   │   ├── Cargo.toml                               ~ feature runtime-wgsl, optional naga and naga_oil
│   │   └── src/
│   │       ├── entry/mod.rs                         ~ from_grid_spec, register_gpu_grid_model over it
│   │       ├── entry/set.rs                         ~ check_id_grammar
│   │       └── gpu/
│   │           ├── mod.rs                           ~ compose, runtime_grid, grid_spec pub(crate)
│   │           ├── compose.rs                       + compose_wgsl, directive check, span mapping
│   │           ├── runtime_grid/
│   │           │   ├── mod.rs                       + GridRule, RuleState, RuleDeclaration, CompiledGridRule
│   │           │   ├── template.rs                  + the template, display and reduce shaders
│   │           │   ├── checks.rs                    + declaration, text and IR checks
│   │           │   └── life_step.wgsl               + the Life starter's step
│   │           ├── grid_spec.rs, grid_engine.rs     ~ step_params_bytes takes the seed
│   │           └── tests/
│   │               ├── mod.rs                       ~ compose, runtime_grid
│   │               ├── compose.rs                   + parity with henad-compute's shaders
│   │               └── runtime_grid.rs              + fifteen rule tests
│   ├── henad-models/
│   │   ├── Cargo.toml                               ~ dev-dependency on henad-compute with runtime-wgsl
│   │   └── src/tests/
│   │       ├── mod.rs                               ~ runtime_parity
│   │       └── runtime_parity.rs                    + parity, mutation corpus, Life parity, kit
│   ├── henad-explore/
│   │   ├── Cargo.toml                               ~ feature model-files
│   │   ├── specs/models/life_rule.toml              + the example model file
│   │   └── src/
│   │       ├── lib.rs                               ~ mod model_file
│   │       ├── model_file.rs                        + ModelFile, ModelFileError
│   │       └── tests/{mod.rs, model_file.rs}        ~ + six model file tests
│   ├── henad-app/Cargo.toml, henad-cli/Cargo.toml   ~ henad-explore with model-files
│   └── henad/
│       ├── Cargo.toml, README.md                    ~ feature model-files, docs.rs features
│       ├── src/lib.rs                               ~ henad::rules, CodeDiagnostic, check_choice_options
│       └── tests/facade_paths.rs                    ~ named_only names the rule items
└── docs/
    ├── reference/api.md, reference/primitives.md   ~ henad::rules, the template's helpers
    ├── authoring/shaders.md                         ~ runtime composition
    └── developing/
        ├── releasing.md                             ~ model-files in the packaging check
        └── agent-record/
            └── 20261010-53-runtime-grid-rules.md    +
```

## State after

Everything is uncommitted on `49/m4-runtime-grid-rules`, on top of `f2d7760`, in the worktree `.claude/worktrees/49-m4`.
Nothing was committed, pushed or tagged.

All checks ran with `HENAD_REQUIRE_GPU=1` on this Mac (M4 Pro).

- `./check.sh` passes on the final tree, after the review fixes: the wasm32 typechecks, fmt of the workspace and the excluded packages, packaging, cargo-deny, clippy, 1299 Rust tests and doc tests with none failed and 6 doc tests ignored as before, the README program, the docs with `-D warnings`, the scripts' tests and the web build.
  The one warning is the future-incompatibility note on naga, wgpu, wgpu-core and wgpu-hal, as on `master`.
  Before the review, of three full runs, the first stopped at `a_location_survives_a_panic_on_a_rayon_worker` alone, the test that flakes at the base commit, and the other two passed whole.
  The run after the fixes passed whole at the first attempt.
- `./scripts/check_hot.sh` passes on the final tree: the lints of the excluded packages, `hot-check` with every check `ok` (the twelve plugin models stepping as the static ones and the kit passing on them included), then `hot-check relink`.
- `cargo +1.95 check --locked -p henad --features example-models,app,cli,testing,hot,model-files` passes on the final tree.
  Clippy with atomics on the pinned nightly over henad-app and over the facade with `app`, `example-models` and `model-files` passed before the review.
  The fixes touch no code behind `target_feature = "atomics"`, and it did not run again.
- `cargo about generate about.hbs` writes the committed `docs/license.html` byte for byte, and `cargo deny --locked check` passes.
  The review fixes add no dependency, and `Cargo.lock` is as before them.
- Among the workspace tests and passing unchanged: `schema_hashes_are_unchanged_since_0_2_0`, `a_0_2_0_manifest_still_resumes`, `a_0_2_0_manifest_still_replays_and_merges`, henad-cli's `tests/golden.rs` and `tests/json_lines.rs`, the tutorial's `snippets`, `parity` and `shaders` tests, `the_readme_program_matches_the_example`, and the byte-identity sweep and track tests, every GPU grid model now boxing the engine itself.
- `uv run zensical build` reports no issues on the final tree, with this record linked from the navigation.
  The `site/` and `.venv/` it left in the worktree, both gitignored, are deleted.
- The scratch directory `/Users/micfong/.claude/jobs/3144150c/tmp/m4/` held the web builds' `.wasm` files and logs, the raw naga_oil experiment's output, the backups of the two henad-app files the size probe changed, the check logs, and the scripts that edited this record and AGENTS.md during the review fixes, and is deleted.
  No template copy was built: the template names no item that changed.

The app was not run.
Nothing in henad-app or henad-cli calls the new code: the Editor tab (M6) and `--model-file` (M7) are its first callers.

M4's plan gate is the Life rule's parity and the kit, which `the_life_starter_matches_game_of_life` and `a_grid_rule_conforms` hold.
M4 has no benchmark gate, and no timing ran.
No per-tick path of a compiled model changed: a compiled GPU grid model now boxes the engine it already forwarded to, and its spec is read once per entry in place of once per `demand`.

Proposed commit: `feat: runtime grid rules and the model file`.

### Benchmark, for a quiet machine (optional)

M4 has no benchmark gate, and no timing ran in this session, since other milestones were building in parallel.
One per-tick path of a compiled model moved: the runner's call into a GPU grid model now reaches `GpuGridEngine` directly, without the adapter's forwarder.
The commands below compare the base and this tree on `gpu_game_of_life` and `gpu_sir`, interleaved, and expect them flat.
The `--export-stats` CSVs and the sweep's `series.csv` and `summary.csv` are expected to match byte for byte.

```bash
# Once, from the root of a checkout holding this change: an old tree at the base commit and both release binaries.
NEW_TREE=$(git rev-parse --show-toplevel)
git worktree add /tmp/henad-m4-old f2d7760
(cd /tmp/henad-m4-old && cargo build --release -p henad-cli)
(cd "$NEW_TREE" && cargo build --release -p henad-cli)
OLD=/tmp/henad-m4-old/target/release/henad-cli
NEW=$NEW_TREE/target/release/henad-cli

# Interleaved, three rounds, flat-out GPU steps at three grid sizes.
for round in 1 2 3; do
  for model in gpu_game_of_life gpu_sir; do
    for size in 64 1024 4096; do
      $OLD $model --steps 2000 --reps 5 --set grid_width=$size --set grid_height=$size --json \
        > "old-$model-$size-$round.jsonl"
      $NEW $model --steps 2000 --reps 5 --set grid_width=$size --set grid_height=$size --json \
        > "new-$model-$size-$round.jsonl"
    done
  done
done

# A blocking stats sample on every tick, and the same series from both builds.
for model in gpu_game_of_life gpu_sir; do
  $OLD $model --steps 2000 --export-stats "/tmp/old-$model.csv"
  $NEW $model --steps 2000 --export-stats "/tmp/new-$model.csv"
  cmp "/tmp/old-$model.csv" "/tmp/new-$model.csv"
done

# A GPU sweep on both builds.
for build in old new; do
  bin=$OLD; [ "$build" = new ] && bin=$NEW
  rm -rf "/tmp/m4-sweep-$build"
  $bin gpu_sir --vary infection_rate=0.1:0.5:0.1 --reps 3 --steps 500 --out "/tmp/m4-sweep-$build"
done
cmp /tmp/m4-sweep-old/series.csv /tmp/m4-sweep-new/series.csv
cmp /tmp/m4-sweep-old/summary.csv /tmp/m4-sweep-new/summary.csv

git worktree remove /tmp/henad-m4-old
```

## Issues found & future directions

- **The work limit bounds work, not time.**
  65536 expressions and statements per call of `step_cell` keeps a rule finite and its pipeline small enough to build, and leaves Life's 1196 fifty-fold room.
  A rule near the limit on a 16384 by 16384 grid can still take seconds a tick, and 64 such steps in one submission can trip the OS watchdog that AGENTS.md's GPU traps describe.
  The editor of M6 can show a rule's cost beside the limit.
- **A problem in a type of the rule has no place.**
  `var x: f64 = 1.0lf;` fails naga_oil's validation as "Type [9] '' is invalid", whose labels carry no span in the text, and the diagnostic has no line.
- **The composer runs twice over a rule's entry.**
  naga_oil preprocesses the text, and `Composition` runs its public `Preprocessor` a second time to recover the text naga parsed, since naga_oil returns no positions for a module that composed.
  It also builds its regexes per composition.
  A test build composes a stock shader in about 3 ms, and a compile on each keystroke in the editor is comfortable at that.
  No timing ran in this session.
- **The span mapping depends on naga_oil internals.**
  It relies on naga_oil keeping lines, decorating only identifiers, the public `Preprocessor` and `Tokenizer` doing what `make_naga_module` does, and the anchor function's span starting at its `fn`.
  `naga_oil_span_encoding_is_unchanged` and the place tests pin each, and fail on a naga_oil that changes them.
  record #49's research notes that Bevy 0.20 dropped naga_oil for WESL, and a naga_oil that falls behind wgpu 31 would block the upgrade.
- **naga_oil's validation runs with `Capabilities::default()`, the writer's with `all()`.**
  A rule therefore meets the WebGPU baseline's capabilities: `f16` and `f64` fail with naga's message.
  The browser compiles with Tint, and a rule naga accepts can still fail there, as design.md §1.5(a) expects.
- **The grid rule joins no sweep track case yet.**
  design.md listed the rule beside `interleaved_gpu_runs_match_sequential_ones` and `a_gpu_search_writes_the_same_tables_on_any_track_count`, which the plan does not carry.
  A rule replays exactly, and adding it there would hold its sweeps to the byte-identity line.
- **`GpuGridState<M>` is no longer the boxed type of a registered model.**
  Its public API is unchanged, and the facade, the tutorial, the guide and henad-models' adapter tests still build it directly, so its forwarders stay exercised.

### Deviations from the plan

- **Two parity tests, not one.**
  The plan asks for every compute shader of henad-compute and henad-models in one henad-models test.
  henad-compute's `shader_bindings` are crate-private, so henad-compute's seven entries are compared in henad-compute, under the design's name `runtime_composition_matches_build_time_shader_strings`.
- **Parity includes wgsl_bindgen's leading line break.**
  `SHADER_STRING` is `"\n"` plus the composed text, from wgsl_bindgen's raw-string literal, and the tests compare that string whole.
- **The template comes before the rule's text.**
  design.md's sketch put the user text between a prefix and `main`.
  With the template first and the text last, the text's lines keep their numbers after the template's, an unclosed brace reports at the end of the rule's text rather than in the template, and naga_oil's import substitution reaches the text's lines the same way it reaches the template's.
- **The template imports from the shared modules under `henad_` aliases** (`pcg_hash`, `offset_cell`, `TORUS`, `BOUNDED`), so the rule's random stream and boundary use the parity-tested twins, and a rule can import or define the plain names itself.
- **The seed reaches the uniform block through `GridHooks`.**
  The hook had no seed, and `step_params_bytes` gained one, which model types ignore.
- **`register_gpu_grid_model` shares one spec.**
  Record #52's open question, taken up as the brief allowed, since it changes no behaviour.
- **`ModelFile` has `from_rule`.**
  The plan lists `parse`, `to_toml`, `rule`, `code`, `entry` and `from_code`, and a rule built in code needs a way into a file.
- **No `ModelFile::read` and no file names.**
  `parse` takes text, a diagnostic names no file, and a host sets the file it read from.
- **`ModelFileError` boxes its single diagnostics**, to keep `Result<_, ModelFileError>` under clippy's `result_large_err` in a caller's crate.
- **`GridRule::check_declarations` and `RuleDeclaration` are public.**
  The plan lists `compile` alone.
  A model file places a declaration problem at its table only when it knows which declaration the problem is about, and henad-explore is another crate.
- **`check_choice_options` is new public API in henad-core.**
  `choice_param` and the two run-time paths, the rule's checks and the model file's `parse`, apply the same three rules, and the facade lists it with the other helpers.
- **`DiagnosticSeverity` has the one variant `Error`.**
  The brief asked for the type, and nothing produces a warning or a note yet.
  It is `#[non_exhaustive]`, and a later variant breaks nothing.

### Open questions for the maintainer

- Should `model-files` stay forced on in henad-app and henad-cli?
  They turn it on now, as the brief asked, and the linker drops it while nothing calls it.
  Once M6 calls the composer, every web app built on the template's `app` feature carries the 0.50 MB gzipped, naga_oil, regex and a second `codespan-reporting` (0.12 beside naga's 0.13) included, with no way to leave it out: the facade's `model-files` then gates the `henad::rules` re-export alone.
  One review proposed a default-on `model-files` feature in henad-app and henad-cli that forwards to henad-explore's, taken by the facade's `model-files` alone, so the official binaries keep it and a template app can leave it out.
  M6 would then gate the Editor tab on it.
- `GridRule::CODE_FORMAT` puts the template's version in the code's format, `henad-grid-rule/1`.
  A template change that keeps every rule's results, such as a renamed internal, need not bump it.
  Is a hash of the composed template for a fixed rule, pinned by a test as design.md §1.6 proposed, wanted on top, to catch a change that forgot the bump?
- A rule's `step_cell` gets no cell position.
  `henad_cell` holds it, under the template's private prefix.
  Should the template offer `cell_x()` and `cell_y()`, or a `position()` helper, for rules that depend on place?

<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
