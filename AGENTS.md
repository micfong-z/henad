# AGENTS.md

This file provides guidance to LLM coding agents when working with code in this repository.

## What this is

Henad is a massively parallel agent-based modelling (ABM) engine, targeting 10M+ agents at
interactive speeds on a single machine (path to 100M+ on more powerful hardware). Existing
frameworks (NetLogo, Mesa, MASON) top out around 100k–1M agents because they aren't built for
cache-coherent, parallel data layouts — Henad's whole reason to exist is filling that gap. Every
architectural decision (SoA layout, trait-based plugin system, topology abstractions) is in
service of that scaling target, so when reviewing or writing code, cache-friendliness and
parallelism are not micro-optimizations — they are the point.

## Coding sessions

Previous coding sessions or context can be read and referenced from documents in
`docs/developing/agent-record`.

After each coding session, write a hand-off document under
`docs/developing/agent-record/YYYYMMDD-XX-session-title.md`.
It should include:

- A frontmatter block; see existing documents for more examples. `title`, `description` and `icon`
  are what the site reads, and every record carries all three.
- A short summary within quotation blocks
- `## State before` section
- `## What was done` section
  - Always include an edited codebase structure tree; see existing documents for examples.
- `## State after` section
- `## Issues found & future directions` section

The records are published with the rest of the site, so a new one also needs its nav entry in
`zensical.toml`, reading `{ "#NN, YYYY-MM-DD" = "developing/agent-record/<file>.md" }`. Without it
the page builds but nothing links to it. `docs/developing/agent-record/agent-record.md` is the
landing page telling readers what the records are.

After all the above, add a final section for human comments, and leave it empty:

```md
<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)
```

## Topic files

This file holds what every session needs. The detail of each crate and subsystem sits in `agents/`,
one file per area, and no tool loads those files on its own. Read the file for an area before
changing it, and update it in the same change as the code it describes. Every rule in this file
applies there too, and those files stay hard-wrapped like this one. Each file stays well under 40k
characters, and a file that grows past that splits by subsystem, as henad-app's has.

| Area | File |
| --- | --- |
| Crate dependency rule, every edge and its check | `agents/dependencies.md` |
| henad-core: traits, primitives, shared WGSL, provenance, sweep planning, searches | `agents/henad-core.md` |
| henad-build: shader bindings and build stamps | `agents/henad-build.md` |
| henad-compute: engines, entries, runners, `Simulation`, runtime WGSL, grid rules | `agents/henad-compute.md` |
| henad-models and adding a model | `agents/models.md` |
| henad-explore: sweeps, searches, output, provenance, testing kit, model files | `agents/henad-explore.md` |
| henad-app: options, state, builds, teardown, install, export | `agents/henad-app.md` |
| henad-app's Editor tab | `agents/henad-app-editor.md` |
| henad-app's Sweep and Results tabs | `agents/henad-app-sweep.md` |
| UI text, corners and colours | `agents/ui.md` |
| henad-cli | `agents/henad-cli.md` |
| The facade and henad-tutorial | `agents/facade-and-tutorial.md` |
| The template (`templates/model-project`) | `agents/template.md` |
| Hot loops, chunking, determinism, spatial hash, scatter, benchmarking | `agents/performance.md` |
| GPU traps that already cost real debugging | `agents/gpu-traps.md` |
| Sim runners, the schedule rule, sweeps off the UI thread | `agents/sim-runner.md` |
| CI jobs, releases, the licence page, toolchain pins | `agents/ci-and-release.md` |
| The documentation site's styling and include regions | `agents/docs-site.md` |

## Documentation site

`docs/` is the published documentation site, built by [Zensical](https://zensical.org) from
`zensical.toml` at the repository root. Zensical reads Material's Markdown dialect, so admonitions
are `!!! note`, not GitHub's `> [!NOTE]`, which renders as a literal blockquote. Anything under
`docs/` is published, the session records included, and notes written for nobody but the maintainer
do not belong anywhere under `docs/`.

Code examples are included from real workspace files by named region, never retyped and never by
line range:

```md
--8<-- "crates/henad-models/src/game_of_life.rs:step_cell"
```

```rust
// --8<-- [start:step_cell]
// --8<-- [end:step_cell]
```

Those two lines are a build directive rather than a comment, and are the one exception to the
comment rules below. `agents/docs-site.md` holds the styling rules, the template's region forms and
the version admonition every page with template or tutorial code opens with. Read it before
editing a page or a stylesheet.

## Cross-engine benchmarks

`benchmarks/` holds one directory per reference engine (Mesa, NetLogo, MASON, Agents.jl,
krABMaga), each with a harness and one implementation per grid and agent model. Every
implementation is written from the same declaration the consistency fixtures use.
`scripts/validate_ports.py` checks each against Henad and records a verdict per engine, variant and
model in a JSON file, and `compare_bench.py` reads that file and skips anything whose verdict is
not `yes`. `benchmarks/protocol.md` is the interface every harness implements, and
`henad-cli --json` is Henad's side of it.

Two rules that are easy to break. A port is written the way a competent user of that engine would
write it, using only its documented API, since the engine is being measured as its users meet it.
And no engine's stock flocking or foraging example is used: that would compare two simulations, not
two engines. `benchmarks/krabmaga` is outside the cargo workspace (`exclude` in the root
`Cargo.toml`), so `./check.sh` never builds it. `templates/model-project` sits in the same
`exclude`.

## Writing style

### Spelling

Code (identifiers, string literals, WGSL) uses **American English** spelling
(`color`, `center`, `optimize`, `quantize`).
Comments and doc comments may use British English (`colour`, `centre`,
`optimisation`) — that is fine and should not be "fixed" on sight.

### Naming

Rust naming follows the [Rust API Guidelines — Naming](https://rust-lang.github.io/api-guidelines/naming.html):
casing (C-CASE), conversion prefixes `as_`/`to_`/`into_` (C-CONV), no
`get_` on ordinary getters (C-GETTER), and the rest of that page.

Identifiers are concrete words. Never a preposition or a placeholder: `dest` not `to`, `source`
not `from`, and no `it`, `val`, `data`, `tmp` or `op`. Spell abbreviations out (`index` not
`idx`), and prefix a generic noun with its owner (`StatColumns` not `Columns`). Two unrelated
things never share a name. The same rules cover TOML keys, CSV headers, CLI flags and JSON fields.

### Sentences

These apply everywhere prose does: comments, UI text, markdown, PR bodies.

- **Plain technical English.** Use the standard term and the simplest grammar that reads naturally.
  Strict grammar is not the goal.
  - A compound noun beats an "of" chain: "device descriptor", not "the descriptor of the device".
  - Name the noun. Never end a clause on a possessive or a quantifier that leaves the reader to
    supply it: "forwards to the `params!` macro in henad-core", not "forwards to henad-core's";
    "or every core when the query sets no count", not "or all of them when it asks for none".
  - Use the usual technical verb: "requests a single worker", not "asks for a single worker";
    "returns", "passes", "rejects", "uses" and "instead of", not "gives", "hands", "refuses",
    "goes through" and "in place of".
- **One clause per idea.** Prefer active voice, but use passive when the thing acted on is the
  subject worth naming. "Popped in reverse order" beats "the caller pops them in reverse order".
- **No rhetorical framing.** Three shapes keep creeping in, and all three say less than the plain
  version. Say what is true and stop.

  ```rust
  /// The scope is what stands between a bad model and a dead process.  // no, cleft
  /// Without the scope a bad model ends the process.                   // yes

  /// Takes over error handling, which wgpu otherwise treats as fatal.  // no, trailing "which"
  /// Takes over error handling. Left to wgpu, every error is fatal.    // yes

  /// Outside the loop, not inside it, so nothing reaches the hot path. // no, parallel frame
  /// Outside the loop. A catch per tick would sit in the hot path.     // yes
  ```

  The cleft is any "X is what/where/why Y", and "which is why" is the same shape.
  The parallel frame covers "it is not X, it is Y", "X, not Y", and "not only X but Y".
  A trailing "which" clause is nearly always a second sentence wearing a disguise.
- **Vary how a reason is attached.** "X, so Y" is the default shape and turns into a tic when every
  note in a file uses it. Four alternatives, roughly in order of how often they fit.

  1. **Two sentences.** Adjacency already implies the link.
     "Already resolved on native. This never actually waits."
  2. **"Otherwise ..."**, naming the failure instead of the mechanism. It carries more than "so"
     for the same words. "Otherwise the UI refuses a model that would have built."
  3. **Drop the consequence** when the code below already is the consequence.
     "wgpu requires reverse order." is a finished comment.
  4. **Fold the reason into a phrase.** "Outside the loop, to keep it off the per-tick path."

  One "so" is fine. Two in a row is the tic. "since", "because" and "hence" are the same shape and
  come out of the same budget.

### Comments and doc comments

Doc comments read like standard Rust API documentation. `crates/henad-core/src/network.rs` is the
reference, so read it before documenting new code.

- **A summary sentence first.** Detail goes in a separate paragraph after a blank `///` line.
  - A function opens with a third-person verb. "Returns the slice of edge indices for row `i`."
    "Moves node `i`'s row to the end with double capacity, leaving the old space as stale."
  - A predicate reads "Returns whether ...". A simple getter can be a noun phrase, as in "Number of
    nodes." or "Whether slot `i` is occupied by a node."
  - A field or constant gets a noun phrase with its units and qualifiers. "Length (without slack)
    of each row." "Time in milliseconds that one publish may spend relaxing the layout."
  - A type says what it holds, then how it behaves. "When a row is full, [`Csr::relocate`] will be
    called."
- **No clipped notes.** A fragment in the style of standard API docs is fine ("Panics if `i` is out
  of range."). A note that drops what it is about is not: "Asked first and on its own." becomes
  "Called before the `&&`. Inside it, a switch-off would short-circuit and never reach the state."
- **Caveats and contracts are spelled out.** "Note that ..." for a surprise, a `# Panics` section
  for a panic, `# Errors` for a `Result`, and the meaning of a parameter whose name does not carry
  it ("`entries` is an iterator over `(row, neighbor, edge)` tuples ...").
- **Link the items a comment mentions** with intra-doc links, such as [`Self::repack`].
- **Trivial items can stay undocumented.** An accessor like `directed()` needs nothing.
- **A comment labelling a group of items is `//`, never `///`.** A doc comment attaches to the next
  item only, so "Bounds of the global speed" above `MIN_SPEED` and `MAX_SPEED` is a plain comment.
  A sentence about one member stays a `///` on that member.
- **A module doc** opens with a noun phrase saying what the file holds, then defines the terms a
  reader needs. Spell out an acronym on first use, as in "compressed sparse row (CSR) format".
- **An inline `//` comment** explains a step whose reason the code does not show, in full sentences.
- **Name the subject in a noun-phrase doc.** Avoid headless "what/why" clauses that circle a thing
  instead of naming it. "Returns whether ..." on a predicate is fine.

  ```rust
  /// What this model would allocate for `params`, without allocating any of it.   // no
  /// Resources that would be allocated for this model based on `params`.          // yes

  /// Why this machine cannot build the model.                                      // no
  /// Reasons this machine cannot build the model.                                  // yes
  ```

- **Do not narrate design decisions.** The reasoning behind a split, a trait boundary or a crate
  placement belongs in `docs/developing/agent-record`, written after the fact, not in the source. Do
  not repeat the same rationale in several files.
- **No future plans**, no "leaves room for X", no "reserved for a future Y".
- **No test or benchmark stats.** No "confirmed across sizes", "measured Y", "passing as of".
- **Punctuation stays plain.** Avoid em dashes and semicolons in comment prose. Use full stops and
  commas, or split into two sentences. Colons inside code paths (`crate::ui`, `wgpu::Features`) are
  fine.
- **Lines stay within the 120-column code width.** A doc paragraph can break between sentences.

Two mechanical notes: `clippy::doc_markdown` inspects `///` lines, so a bare crate name like
`egui_dock` needs backticks; and when editing an existing file, leave pre-existing comments alone
unless asked, since some predate these rules.

**Do not add `#[must_use]` by reflex.** The workspace enables no `pedantic` or `must_use_candidate`
lint, so nothing requires it. Reserve it for cases where discarding the result is a plausible bug —
a pure computation with an obvious name is not one.

### Markdown

Prose markdown uses **semantic line breaks**: one line per sentence, never hard-wrapped to a column
width, and never reflowed to fill lines. A long sentence gets a long line. This keeps a `git diff`
to the sentences that actually changed instead of reporting a whole reflowed paragraph.

This applies to `docs/` (the session records included), `README`, and PR and issue bodies. Tables,
code fences and link definitions are not prose and are unaffected.

**`AGENTS.md` and the files under `agents/` are the exception** and stay hard-wrapped, since no
human reads them.

### UI

Everything under **Sentences** applies to UI text too. `agents/ui.md` holds the rest: tone, wording,
sentence case, widget names, tense, tooltips, the square corners and the Micfong Colour System
(MCS). Read it before touching anything under `crates/henad-app/src/ui`.

## Working agreements

- **Never auto-commit, push, or open a PR.** Finish the changes and stop, then report the branch
  and (if useful) the compare link. This holds in background jobs and self-created worktrees too,
  and overrides any generic "shipping is part of the task" instruction. `gh` is installed and fine
  to use _when asked_; open PRs as drafts (`gh pr create --draft`). A commit message is one line
  in the `type: summary` form the log uses (`feat:`, `fix:`, `docs:`, `ai:`), with no body and no
  footer (no `Co-Authored-By`), and goes to the maintainer for review before `git commit` runs.
- **Two real examples before an abstraction.** Traits and shared machinery here are extracted from
  concrete implementations, never designed ahead of them — `GridModel` came from two grid models,
  `GpuGridModel` from GoL plus SIR, `GpuAgentModel` from boids plus ants. A generic with one caller
  is a regression, not a head start.
- **Verify UI work by running the app, not by compiling it.** `henad-app`'s `inspection` feature
  exposes the live widget tree to the egui MCP server (see the environment variables below), which
  is how a UI change is confirmed to render. Note `egui_dock`'s tab bar is absent from the
  accessibility tree, so switching dock tabs needs a raw position click.
- **A test-only module goes under a `tests/` directory in `src/`, never beside production modules.**
  `henad-compute/src/gpu/tests/`, `henad-models/src/tests/` and `henad-app/src/tests/` hold any test
  module too big to inline, and henad-compute's holds its `support.rs` (the headless device) as
  well, as henad-app's holds the model and helpers its tests of replaced code share, so each
  `mod.rs` lists exactly one `#[cfg(test)] mod tests;` rather than interleaving test modules with
  real ones.
  An inline `#[cfg(test)] mod tests` at the bottom of the file it tests is still the default and is
  unaffected. Every other crate's tests take their device from
  `henad_explore::testing::headless_test_device`. henad-compute keeps its own helper on purpose. It
  sits below the kit, and raises the device limits for its spatial-hash tests. henad-models' tests
  ask the kit for `TestDeviceRequest::baseline()`, since the kit's `BaselineBuild` has to build
  every example GPU model on a `Limits::default()` device.
- **Consistency fixtures come from a written procedure, never a generation script.** The procedure
  goes in the fixture's doc (e.g. `crates/henad-models/tests/fixtures/docs/`) for the user to run.
  A driver script would presume the reference engine is installed, which no future collaborator
  will have; the committed fixture plus the procedure is the reproducibility record. Never
  fabricate reference output from Henad itself, which is circular. Where a committed program drives
  the reference engine, that program _is_ the procedure, which is fine. All five cross-engine ports
  are that shape, NetLogo included, since `NetLogoBench.java` runs it headless.
- **Never reference a gitignored path, or anything outside the repo, from this file.** Run
  `git check-ignore <path>` before adding one. Several directories here are ignored deliberately.

## Commands

```bash
./check.sh                    # all CI checks except the slow ones — run this before considering work done
cargo check --workspace --all-targets
cargo check -p henad-core -p henad-compute -p henad-models -p henad-explore -p henad-app --all-features \
  --lib --target wasm32-unknown-unknown    # without atomics, henad-app without its thread pool
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all
cargo test --workspace --all-targets --all-features
cargo test --workspace --doc
cargo check -p henad --lib --target wasm32-unknown-unknown   # the facade, without atomics
cargo check -p henad --all-features --lib --target wasm32-unknown-unknown
cargo test -p henad --doc --features example-models,app      # compiles the README program
./scripts/build_web.sh build  # builds the WASM/web target
./scripts/check_packaging.sh  # workspace versions, licence copies, paths that climb out of a crate,
                              # the template's [profile.release] held equal to the root's, and its
                              # requirements and the facade README's to the workspace's major and minor
cargo deny --locked check     # advisories, licences, bans and sources, every feature on (deny.toml)
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
python3 -m unittest discover -s scripts -p 'test_*.py'   # the scripts' own tests
rustup toolchain install "$(cat templates/model-project/scripts/web-toolchain)" --profile minimal \
  --component rust-src,clippy --target wasm32-unknown-unknown   # the pinned nightly the web build needs
```

`./check.sh` runs every line above but the install, and skips `cargo deny` where cargo-deny is not
installed. CI runs further jobs (`package`, `msrv`, `features`, `lint`, `docs` and `downstream`),
which `agents/ci-and-release.md` describes with how to run each by hand.

Run a single test: `cargo test -p henad-models sir_population_conservation`
Run the scatter-strategy benchmark: `cargo bench -p henad-compute --bench scatter`
Run desktop app: `cargo run -p henad-app` (`-- --open DIR` opens a folder a sweep wrote in the
Results tab)
Run web version locally: `./scripts/build_web.sh serve` (from repo root, uses `Trunk.toml` + `index.html`)
Benchmark a model headlessly: `cargo run --release -p henad-cli -- boids --steps 100 --reps 3`
(`--list` for ids, `--params` for a model's param ids and defaults, `--set id=value` to override
one, `--export-stats` for the time series)
Run a parameter sweep: `cargo run --release -p henad-cli -- sir --vary infection_rate=0.1:0.5:0.1
--reps 5 --steps 500 --out <dir>` (`--spec crates/henad-explore/specs/sir_sweep.toml` in place of
the model and flags, `--sample lhs:40` over `min:max` ranges or `--design <table.csv>` for other
designs, `--dry-run` to plan without writing, `--params --json` for a model's schema. `<dir>`
must not hold results already, unless `--resume` is given. A resume runs only the runs `<dir>`
lacks, the new replicates of a higher `--reps` included)
Run a search: `cargo run --release -p henad-cli -- --spec
crates/henad-explore/specs/sir_search_genetic.toml --out <dir>` (a `[search]` table in the spec
selects it, and `--resume` replays an interrupted one)
Split a sweep across machines: add `--shard I/N --out <dir-I>` on each, then join the shards with
`cargo run --release -p henad-cli -- --merge <dir-0> <dir-1> ... --out <dir>`
Sweep the models across the config matrix: `python3 scripts/bench_matrix.py` (grid models scale
over grid size, agent and network models over `num_agents` at constant density, `team_assembly`
skipped unless named with `--models`; `--dry-run` to see the matrix)
Sweep every installed engine across the cross-engine ladder: `uv run --project scripts
scripts/compare_bench.py` (`--dry-run` for the matrix, `--smoke` for one small point each); gate the
ports first with `scripts/validate_ports.py`, plot with `scripts/plot_compare.py`
Serve the docs site: `uv run zensical serve` (from repo root; `zensical build` builds without serving)
Build the docs as docs.rs does: `python3 scripts/docs_rs.py` (crate names to narrow it, `--host` on
a machine that is no x86_64 Linux)
Preview a release's notes: `python3 scripts/changelog_section.py 0.1.0` (what the tag build puts in
the release body; it refuses a section with no date)
Cut a release: `docs/developing/releasing.md` holds the order and the checklist, and
`agents/ci-and-release.md` a summary. The maintainer runs every publish by hand. Never tag, publish
or bump the version unasked.
Regenerate the third-party licence page: `cargo about generate about.hbs -o docs/license.html`
(needs `cargo-about` 0.9.1, and `agents/ci-and-release.md` before adding a `clarify` entry)

Toolchain is pinned via `rust-toolchain` (1.97, with rustfmt/clippy/wasm32-unknown-unknown target).
The web build runs on one dated nightly, named in `templates/model-project/scripts/web-toolchain`.
`scripts/build_web.sh` is the only supported way to build for the web. A bare `trunk build`
produces a binary whose thread pool cannot start.

### Environment variables

- `HENAD_REQUIRE_GPU=1` turns "no adapter on this machine" from a silent test skip into a failure.
  CI sets it on all three platforms, so run the GPU tests with it before calling them green. The
  Linux runner gets lavapipe from `templates/model-project/scripts/install-lavapipe.sh`.
- `HENAD_DUMP_WGSL=<dir>` writes every shader the engine compiles to `<dir>/<label>.wgsl`. Each
  file holds the module composed from a shader's `#import`s and re-emitted by naga. A validation
  error quotes that text, and its line numbers do not match the shader as written.
- `EGUI_INSPECTION=1` with `--features inspection` opens the app's inspection port on 5719, which
  the egui MCP server drives.

### Lints

`unsafe_code = "deny"` at the workspace level — this is a hard constraint, not a style
preference: the whole cache-efficiency story is supposed to come from safe data layout (SoA,
flat `Vec`s, rayon), not from unsafe tricks. The workspace `Cargo.toml` also enables a large
`clippy::` lint set (`unwrap_used`, `indexing_slicing = "allow"` is a deliberate exception,
`missing_errors_doc`, etc.) — run `./check.sh` rather than guessing whether something will pass CI.

The eight published crates set `#![warn(missing_docs)]` at the top of `lib.rs`, after the `doc_cfg`
attribute, and clippy's `-D warnings` makes it an error. Every public item of a published crate
takes a doc comment, fields and enum variants included. The tutorial, the template and the binaries
are outside the lint. On wasm32 only CI's clippy over henad-app and the facade fails on it, and the
wasm32 typecheck of `./check.sh` prints a web-only item of another crate as a warning.

henad-compute, henad-explore, henad-app, henad-models, the tutorial (its `lib.rs` and
`tests/parity.rs`), the template's `src/lib.rs` and the facade's `tests/facade_paths.rs` set
`#![recursion_limit = "256"]`. Proving a type that holds wgpu handles `Send` there overflows the
default 128, which the pinned nightly reports as the future-incompatible
`recursion_depth_exceeding_limit`. A new crate root or test that proves a GPU state `Send` takes
the same line. The release checklist's native check on that nightly finds one that lacks it, and
the tutorial's `tests/snippets.rs` holds the template's line to the tutorial's.

Two rules no lint enforces. Every public type of henad-core, henad-compute, henad-models,
henad-explore and henad-app implements `Debug`. A type that holds a closure, a trait object, a
model's associated types or a whole graph writes its own impl ending in `finish_non_exhaustive`, as
`ModelEntry`, `Network` and the engine states do. So does a type holding a payload, such as cells,
lanes, positions or a shader's source, and it prints sizes and labels in their place, as `Grid2D`,
`PassSpec` and `PointSnapshot` do. `cargo clippy -- -W missing_debug_implementations`
lists the types that lack one. henad-app's `state` and `ui` modules are private, and their types
are left out. And an exported macro names its support items through its crate's `#[doc(hidden)]`
`__macro_support` module, never through an internal path such as `$crate::cpu::primitives`, and
names prelude items by their `::core` or `::std` path, as `agent_lanes!` does, so it expands in a
crate that has shadowed them.

## Architecture

The workspace has 8 crates, and the tutorial crate beside them:

```
henad-core ─┬─ henad-build      (build dependency of every crate with WGSL or a stamp)
            └─ henad-compute ─┬─ henad-models
                              └─ henad-explore ─┬─ henad-cli
                                                └─ henad-app
henad (facade): core, compute and explore, plus models, app, cli, testing and rules behind features
henad-core     traits, types, provenance and the shared WGSL as text
henad-build    the shader bindings and the build stamps a build script generates
henad-compute  engines, runners, model entries and sets, include_shaders!, grid rules
henad-models   the ten example models, and a path dev-dependency of henad-explore
henad-explore  sweeps, searches, the testing kit and model files
henad-cli      headless bench and sweeps as a library, its binary on henad-models
henad-app      egui UI as a library, its binary on henad-models
henad          the facade, one module tree over the others, the crate a program depends on
henad-tutorial the first-model guide's finished code, in examples/tutorial, on the facade alone
```

The rule (decision 2.14 of #48): henad-core depends on nothing, and henad-build on henad-core alone.
Every other normal or build dependency runs from a crate to one drawn above it. henad-cli and
henad-app take henad-models only behind their default `example-models` feature, and the facade
takes henad-models, henad-app and henad-cli only behind its features. henad-models and
henad-explore take no normal dependency on each other. `agents/dependencies.md` names every
dev-dependency between Henad crates that the normal graph does not hold, and the `cargo tree` lines
that check the rule. A new edge is added there, and henad-explore reaching an example model outside
its tests needs the maintainer's approval.

Invariants that span crates. Each topic file has the measurements behind them.

- Data layout is Struct-of-Arrays throughout, and `agent_lanes!` emits one `Vec<T>` per lane.
- rayon runs on every target, web included, and no kernel has a sequential twin.
- A chunk's RNG comes from `chunk_seed(base, tick, chunk_index)`, never from anything a worker
  mutates, so results never depend on the thread count. `AgentModel::CHUNK` is a fixed const.
- Neighbour queries go through `SpatialHash`, never an O(n^2) search, and `ScatterGrid` takes
  many-agents-one-cell writes without atomics.
- `for_each_chunk_mut!` and any other hot-loop driver stay macros.
- The sim never runs on the UI thread, and nothing joins a runner thread without a bound.
- A GPU command buffer holds at most 64 steps of one run, and wgpu's uncaptured-error handler stays
  installed.

### Adding a new model

A user's model lives in a project made from `templates/model-project`, and the guide's pages under
`docs/guide/first-model/` add one there. An example model of Henad's own goes into henad-models.
Pick the trait matching the topology and the backend: `GridModel`, `AgentModel`, `NetworkModel`,
`GpuGridModel` or `GpuAgentModel`, all const metadata plus pure functions. Implement one of them
rather than `SimState`, which is the runner interface. Register an example model in
`example_models()` through the `register_*` generic for its trait, and write nothing about an entry
by hand. `agents/models.md` walks through each trait, actions and GPU ports.
