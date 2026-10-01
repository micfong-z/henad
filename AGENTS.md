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

After all the above, add a final section for human comments, as

```md
<!-- ─────────────────────────────────────────────────────────────────────────
     EVERYTHING BELOW THIS LINE IS WRITTEN BY THE HUMAN MAINTAINER.
     Agents: do not edit, summarise, reformat, or regenerate this section.
     The one exception is the seed comment below, written once when the record
     is created. Any later pass leaves the whole section alone.
     ───────────────────────────────────────────────────────────────────── -->

## Manual notes (human)

<!-- Seeded by the agent: what the human did this session, from the agent's point of view.
     Raw material to reframe, not notes. Delete this block once rewritten.

     - ...
-->
```

Seed that comment with what the _human_ did: the calls they made, the corrections they gave, the things
they caught that the agent had wrong. It is there so the maintainer can reframe a session into their own
notes without reconstructing it from the transcript, so keep it factual and specific — a decision and the
reason behind it, an intervention and what it changed. Not praise, and not a summary of the agent's own
work, which the sections above already cover. Write it only when creating the record; a later pass leaves
it alone, since by then the human may have started rewriting the section.

## Documentation site

`docs/` is the published documentation site, built by [Zensical](https://zensical.org) from
`zensical.toml` at the repository root. Zensical reads MkDocs Material's configuration format and
Material's Markdown dialect, so admonitions are `!!! note`, not GitHub's `> [!NOTE]`, which renders
as a literal blockquote. Python dependencies are pinned in `pyproject.toml` and `uv.lock`, and CI
builds the site on every pull request and deploys it to GitHub Pages from `master`.

Anything under `docs/` is published, the session records included. Those sit under
`developing/agent-record/` behind a landing page that tells readers what they are. Notes written for
nobody but the maintainer still do not belong anywhere under `docs/`.

Code examples are included from real workspace files rather than retyped, so an example cannot
drift from the code it documents. The include reads

```md
--8<-- "crates/henad-models/src/game_of_life.rs:step_cell"
```

and the named region is marked in the source with a pair of comments:

```rust
// --8<-- [start:step_cell]
// --8<-- [end:step_cell]
```

Those two lines are a build directive rather than a comment, and are the one exception to the
comment rules below. Do not include by line range, which is also supported and drifts silently.

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
`Cargo.toml`), so `./check.sh` never builds it.

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
- **Complete sentences.** Write the subject and the verb out. A clipped note like "Asked first and
  on its own." becomes "Called before the `&&`. Inside it, a switch-off would short-circuit and
  never reach the state."
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

**`AGENTS.md` is the exception** and stays hard-wrapped, since no human reads it.

### UI

Everything under **Sentences** applies here too. On top of it:

- **Neutral tone.** No jargon unless the reader needs the precise word, and define an acronym the
  first time a panel uses one. A term the audience debugs with counts as necessary. A model
  author's kernel "panicked".
- **Conventional UI wording.** Use the words desktop and technical software already uses
  ("integer", "unavailable", "select", "default", "switch", "appear") over home-made phrasing
  ("whole number", "fill in", "stay at their values", "not available").
- **Sentence case.** "Model build failed", not "Model Build Failed".
- **Name a widget exactly as the widget is labelled.** The button reads Build, so prose says
  "press Build", and a field labelled Name is "the Name input field".
- **Tell the user what to do. A widget is never the subject.** "Press Show results to open them",
  not "Show results opens them". "Select another folder without results", not "Start needs a
  folder without results".
- **Tense follows meaning.** Present for a state ("Seed changed."), future for what will happen
  ("The model will be rebuilt.", "Results will appear as runs finish."), imperative for an
  instruction. Keep verbs simple, and avoid "should" and "would".
- **Short tooltips.** Say what the control does or what the user can do next. Leave out how it
  works inside and cross-references to the CLI unless the user needs them to act. "Seed for random
  number generation." is a finished tooltip.
- **No rounded corners.** `init.rs` sets every widget, window and menu radius to zero, and anything
  drawn outside those styles keeps to it: a `ProgressBar` takes `.corner_radius(0)` (egui draws a
  pill by default), and a chip, pill, badge or `Frame` gets `CornerRadius::ZERO`. A plot shows
  through `ui::show_plot`, since `egui_plot` paints its background with a fixed radius. egui gives
  the stripes of an `egui::Grid` a fixed radius of 2 as well. A key/value list goes through
  `ui::kv_grid`, whose rows end with `KvGridRows::end_row` and get square stripes. No other grid is
  striped. `egui_dock` draws the tab bar's scroll bar as a pill, and the dock hides it.
- **Colours come from the Micfong Colour System (MCS).** `ui/mcs.rs` holds one constant per entry
  (`mcs::BLUE_700`), and its module doc maps each state to entries: the state picks the hue, and
  the element carrying the colour picks the step. A badge, a fill or a coloured text takes its
  entry from that mapping in place of a `gamma_multiply` tint, a `Color32` literal or a theme
  colour borrowed as a fill. The theme's warn and error colours are orange 500 and red 500, and a
  status text reads them from the theme. The theme's neutral surfaces and strokes in `init.rs`
  predate MCS and stay as they are. Data colours (the plot series, the heatmap scale and its
  no-data grey) are not MCS and come from `ui/results/plot.rs`.
- **No superfluous politeness.** No "please", no apology.
- **"can" for ability, "might" for possibility.** Never "may".
- **Articles are optional.** "Load model" is as good as "Load the model". Drop the article when it
  buys nothing. In a button, a short label or a tooltip that is most of the time.

```text
The GPU reported an error while building the model.   // no, the article buys nothing
GPU reported an error while building the model.       // yes

Copy the message to the clipboard                     // no, tooltip
Copy message to clipboard                             // yes

Could not build the model                             // no, past modal
Model build failed                                    // yes

Seed changed but not applied. Press Build to apply.   // no, wordy
Seed changed. Press Build to apply.                   // yes

Results fill in as runs finish. Show results opens them.            // no, widget as subject
Results will appear as runs finish. Press Show results to open them. // yes

Varies one parameter at a time. The others stay at their Parameters tab values.    // no
Varies one parameter at a time. Other parameters use values from the Parameters tab. // yes
```

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
  `henad-compute/src/gpu/tests/` and `henad-models/src/tests/` hold their crate's `support.rs` (the
  headless device) plus any test module too big to inline, so each `mod.rs` lists exactly one
  `#[cfg(test)] mod tests;` rather than interleaving test modules with real ones. An inline
  `#[cfg(test)] mod tests` at the bottom of the file it tests is still the default and is unaffected. The two `support.rs` files look like duplicates and are not: henad-compute raises
  the device limits, henad-models deliberately does not, since
  `every_gpu_model_builds_on_a_baseline_device` has to run on a `Limits::default()` device.
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
cargo check -p henad-core -p henad-compute -p henad-models -p henad-explore --all-features --lib \
  --target wasm32-unknown-unknown          # typechecks without atomics; henad-app cannot
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::all
cargo test --workspace --all-targets
cargo test --workspace --doc
./scripts/build_web.sh build  # builds the WASM/web target
./scripts/check_packaging.sh  # workspace versions, licence copies, paths that climb out of a crate
cargo deny --locked check     # advisories, licences and sources
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
rustup toolchain install "$(cat templates/model-project/scripts/web-toolchain)" --profile minimal \
  --component rust-src,clippy --target wasm32-unknown-unknown   # the pinned nightly the web build needs
```

`./check.sh` runs every line above but the install, and skips `cargo deny` where cargo-deny is not
installed. CI alone runs `cargo package --workspace --exclude henad-tutorial --no-verify --locked`
(the `package` job, on manifest changes), `cargo +1.95 check --workspace --locked` (`msrv`), and
henad-app's wasm32 docs on the pinned nightly with the atomics flags in `RUSTFLAGS` and
`RUSTDOCFLAGS` (`docs`).

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
Preview a release's notes: `python3 scripts/changelog_section.py 0.1.0` (what the tag build puts in
the release body; it refuses a section with no date)
Regenerate the third-party licence page: `cargo about generate about.hbs -o docs/license.html`
(needs `cargo-about`, pinned to 0.9.1 in CI; the `lint` job fails when the committed page and the
dependency tree disagree). A crate shipping two files under one licence gets a `clarify` entry in
`about.toml`, as `rfd` and `miniz_oxide` have. Left alone, cargo-about keeps whichever file its
directory walk finds first, and walk order depends on the filesystem. A page generated on macOS can
then fail the check on Linux, or pass it by luck, as `miniz_oxide` did. The app's four fonts are no
crates, and their section of the page is static HTML in `about.hbs`. Their sources, licences and the
procedure that rebuilds Henad Sans and Henad Mono from IBM Plex are in
`crates/henad-app/assets/fonts/SOURCES.md`.

Toolchain is pinned via `rust-toolchain` (1.97, with rustfmt/clippy/wasm32-unknown-unknown target).
The web build is the exception and runs on one dated nightly, named in
`templates/model-project/scripts/web-toolchain` and selected by `scripts/build_web.sh`, which prints
the install line above when the toolchain or its `rust-src` is missing. The Trunk release sits beside
it in `templates/model-project/scripts/trunk-version`. CI, vercel.json and the docs read both files,
and the repository holds one pin. Threads on wasm need
`-C target-feature=+atomics,+bulk-memory,+mutable-globals` and a std rebuilt to match, so the nightly
needs `rust-src`. That script is the only supported way to build for the web; a bare `trunk build`
produces a binary whose thread pool cannot start. It also unsets `CARGO_ENCODED_RUSTFLAGS`, which
would otherwise outrank the `RUSTFLAGS` it sets.

### Environment variables

- `HENAD_REQUIRE_GPU=1` turns "no adapter on this machine" from a silent test skip into a failure.
  CI sets it on all three platforms, so run the GPU tests with it before calling them green.
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

Two rules no lint enforces. Every public type of henad-core, henad-compute, henad-models and
henad-explore implements `Debug`. A type that holds a closure, a trait object, a model's associated
types or a whole graph writes its own impl ending in `finish_non_exhaustive`, as `ModelEntry`,
`Network` and the engine states do. `cargo clippy -- -W missing_debug_implementations` lists the
types that lack one. henad-app is left out, since its public `state` and `ui` modules are app
internals. And an exported macro names its support items through its crate's `#[doc(hidden)]`
`__macro_support` module, never through an internal path such as `$crate::cpu::primitives`, and
names prelude items by their `::core` or `::std` path, as `agent_lanes!` does, so it expands in a
crate that has shadowed them.

## Architecture

The workspace has 7 crates:

```
henad-core ─┬─ henad-build      (build dependency of every crate with WGSL)
            └─ henad-compute ─┬─ henad-models
                              └─ henad-explore ─┬─ henad-cli
                                                └─ henad-app
henad-core     traits, types, provenance and the shared WGSL as text
henad-build    the shader bindings a build script generates
henad-compute  engines, runners, model entries and sets, include_shaders!
henad-models   the ten example models, and a path dev-dependency of henad-explore
henad-explore  sweeps and searches
henad-cli      headless bench and sweeps, also on henad-models for example_models()
henad-app      egui UI, also on henad-models for example_models()
```

The rule (decision 2.14 of #48): henad-core depends on nothing, and henad-build on henad-core
alone. Every other normal or build dependency runs from a crate to one drawn above it:
henad-explore and henad-models onto henad-compute, the hosts onto henad-explore,
henad-compute and, for `example_models()`, henad-models, and any crate with WGSL onto
henad-build. henad-models and henad-explore take no normal dependency on each other, and
`cargo tree -p henad-explore -e normal -i henad-models` prints nothing. Every dev-dependency
between Henad crates that the normal graph does not hold is named here: today the one from
henad-explore to henad-models, for its tests. henad-explore reaching an example model outside
its tests needs the maintainer's approval as a new edge.

- **henad-core**: no dependencies on other crates — not even wgpu or bytemuck, which is why the two
  GPU traits describe their shaders as `&'static str` and their buffers as plain bytes. Defines the
  abstractions everything else builds on. `authoring/` is the model authoring API and splits in two.
  `authoring/model/` holds the five traits a model implements,
  one per (topology × backend): `GridModel` (`authoring/model/grid_model.rs`) for cellular automata,
  `AgentModel` (`authoring/model/agent_model.rs`) for agent populations, `NetworkModel`
  (`authoring/model/network_model.rs`) for nodes joined by edges, `GpuGridModel`
  (`authoring/model/gpu_grid_model.rs`) for shader-resident grids and `GpuAgentModel`
  (`authoring/model/gpu_agent_model.rs`) for shader-resident populations, plus `FieldLayer`
  (`authoring/model/field.rs`),
  the grid slot an `AgentModel` sits over. `authoring/primitives/` is the primitive vocabulary those
  kernels call — wrapping, neighbourhoods, distances, random draws — most paired with a WGSL twin
  under `authoring/primitives/wgsl/`, and each pure one pinned to its twin by a parity test.
  `wgsl/` holds the five shared modules a shader imports as `henad::dispatch`, `henad::dims`,
  `henad::rng`, `henad::space` and `henad::reduce_tree`, as text in `SHARED_WGSL_MODULES`, with
  their `SHARED_WGSL_FNV1A64` computed by a `const` FNV-1a (`Fnv1a64`'s writes are `const fn`).
  An interface change to one ships only in a breaking release. User shaders import these paths.
  `space::offsets`, `space::for_each_neighbor`, `rng::mix_seed` and `rng::next_index` are Rust
  only. `next_index`'s redraw needs a 64-bit product, and WGSL has no 64-bit integers.
  `docs/reference/primitives.md` is the index, marks each Rust-only and WGSL-only entry, and
  records what is deliberately absent.
  `SimState` (`model.rs`) is the _runner_
  interface the sim thread drives, not an authoring API — that split is why the traits live under
  `authoring/model/` and this one does not. `provenance.rs` holds `BuildInfo`, the identity of one
  compiled crate that `build_info!` returns for the crate it expands in, and `ModelSource`, an
  entry's type path and the build of the crate that registered it. Their commit, dirty flag and
  source hash read as unknown, since no build script stamps them yet. Also the `Grid2D<T>`
  double-buffered SoA grid (`grid.rs`),
  the counting-sort `SpatialHash` and the `HashGrid` cell geometry both backends share
  (`spatial_hash.rs`), the `Network` graph a `NetworkModel` works on (`network.rs`), param
  descriptors and `ParamStore` (`params.rs`), `ActionDescriptor`, the `actions!` macro,
  `action_seed` and the `Schedule` of `--act` entries with its `Fire` rule (`action.rs`),
  stat/view types consumed by the UI (`view.rs`), and small shared
  helpers such as the param and stat builders (`helpers.rs`). `xorshift64` sits with the random
  draws in `authoring/primitives/rng.rs`. `Extent` is re-exported at the crate root.
  `Network` keeps its adjacency rows in compressed sparse row (CSR) form with slack, beside an edge
  list the view draws. A full row relocates to the end, the engine repacks after a tick once
  `should_repack()` finds too much stale space, and a full `rebuild()` runs only when the graph
  changes direction. `version()` goes up whenever the edges, their colours or their direction
  change. Those three, `set_directed` and the raw `spawn` and `retire` are `#[doc(hidden)]`, since
  the engine calls them and a model goes through `Nodes`.
  `explore/` holds the parts of a sweep that need no engine, and builds on wasm like the rest of the
  crate. `value.rs` reads a param value written as text and checks it against its descriptor
  (`parse_value`, `resolve_params`, `parse_overrides`), and `format_value` writes one back. `--set`
  goes through it. A choice reads an option's name before its index, and `helpers::choice_param`
  panics on an option name that reads as an unsigned integer or appears twice. `spec.rs` is a
  sweep as written, a `SweepSpec` of `BlockSpec`s with its
  `RunSettings`, `MeasureSettings`, `SeedSettings` and `ActionSpec`s. `SweepSpec::plan` (`plan.rs`)
  checks it against a `ModelSchema` and lists every `Config`, its param values and one tick per
  action, into a `Plan`, where run `run_id` is replicate `run_id % replicates` of config
  `run_id / replicates`. It refuses an unknown id, a value out of bounds, a param both fixed and
  varied or varied twice in a block, a zip of unequal lengths, an `F32` range with no step outside
  a sampled design, a factor with no levels (`DesignError::NoLevels`), an undeclared action, a
  name two actions share, and a stop or `first` threshold that is not finite (`MeasurePlan::check`,
  through `StopSpec::check_threshold` and `Comparison::check`), all before any run. An
  action due past the last tick is only a `PlanWarning`. `Plan::schedule` orders a config's actions
  by tick, then by spec order, and a `Shard` (`I/N`) takes the runs with `run_id % N == I`.
  `factor.rs` resolves a factor over a param or an action's tick (`FactorTarget`) from a list, an
  inclusive range or `all`, and `LevelSpec::parse` reads the level text of `--vary`. It and a plan
  ignore the spaces around each listed value.
  `inclusive_steps` computes value `i` as `min + i * step` and never accumulates, and a sampled
  design takes a range with no step as a whole `FactorDomain`. `design.rs` combines a block's
  factors under a `DesignKind`: `Factorial` (the first factor slowest, the last fastest), `Zip`,
  `Random`, `LatinHypercube` or `Table`. A Latin hypercube gives each factor its own Fisher-Yates
  permutation of the strata. A continuous factor takes one jittered point per stratum, computed in
  `f64` and clamped after the cast, and a factor of `m` levels takes level `stratum * m / N` in
  `u128`, the lowest level its stratum covers, so each level lands in `N / m` configs, rounded down
  or up. That lattice is deliberate. It balances the levels exactly, and the design seed decides
  only which config takes each level. `design_rng.rs` (`DesignRng`)
  draws both sampled designs from a design seed through `xorshift64` and `next_index` in integer
  arithmetic, with a `u64` path for a full `u32` span, and a seed gives the same design on every
  platform. `design_csv.rs` reads a design table, one column per param id or `action.<name>`,
  through `parse_records`. `seed.rs` derives each run's seed from one root through `mix_seed`, and
  `design_seed` a sampled block's when the spec gives none. Under `SeedScheme::Common`, replicate
  `r` of every config shares a seed (common random numbers), and `independent_run_seed` mixes the
  config in. `measure.rs` fixes the sampled ticks, `warmup + j * stats_every` plus the total
  (`MeasurePlan`), and folds each sample into the reducers and a `SeriesBuffer` (`Sampler`),
  skipping and flagging a value that is not finite. `Sampler::push` reports whether the
  `StopCondition` holds at that sample. `stop.rs` reads a condition such as `Infected <= 0`, split
  at the first comparator so a label can hold spaces, and NaN never meets one. `reducer.rs` binds
  the `ReducerKind`s to `StatColumns` indices: final, min, max and mean by default, and argmax,
  argmin, `first<=10` (`FirstCrossing`) and `mean@200..600` (`WindowMean`) on request. `outcome.rs`
  holds `PlannedRun`, `RunStatus` (`timed_out` among the failures), `StopReason` and `RunOutcome`,
  `summary.rs` the replicate statistics of `summary.csv` (`RunningMoments`, `student_t_975`,
  `SummaryAccumulator`), and `fingerprint.rs` the FNV-1a hashes naming a schema, a plan and a run.
  `schema_hashes_are_unchanged_since_0_2_0` (henad-explore's `schema.rs`) pins every example
  model's schema hash to the value 0.2.0 recorded, through the procedure in
  `crates/henad-models/tests/fixtures/docs/schema-hashes-0.2.0.md`. Keep it.
  `replay.rs` holds `Replay`, the model, params, seed, schedule and ticks of one run as a live
  simulation rebuilds it, and `Plan::replay(run_id)` builds one from a plan. `plan_hash` leaves out
  the replicate count and the timeout, and a resume can change both. Unlike `DefaultHasher`,
  FNV-1a is stable across Rust releases. `export/csv.rs` formats and escapes CSV fields and reads
  RFC 4180 records back, and `StatColumns` (`export/stats_csv.rs`) is the column layout
  `StatsWriter` fixes from the first sample. A sweep fixes its `series.csv` columns, reducer
  bindings and stop column with the same type.
  `explore/search/` holds the searches, pure and seeded like the designs. `mod.rs` is the ask/tell
  `Searcher` trait. `ask(max)` returns up to `max` `Candidate`s, each an id, a `Genome`, a
  `replicate_offset` and a `CandidateOrigin`, and `tell` takes their `Evaluation`s sorted by
  candidate id, one row of watched column values per replicate. `SearchSpec` holds the algorithm,
  `max_evaluations`, `batch_size`, an optional `Objective` (`column`, `Goal`, `Aggregate`) and the
  space as `FactorSpec`s, and `SearchSpec::searcher` builds the searcher from `search_seed(root)`.
  `CandidateTracker` numbers the candidates and holds the budget, a count of evaluations with the
  re-evaluations among them. `genome.rs` (`SearchSpace`, `Genome`) resolves the space as a sampled
  design does, keeps a repeated listed level once at its first place (0.0 and -0.0 as one level),
  and decodes a gene `u` in `[0, 1]`: linearly for an `f32` range, and as
  `floor(u * m)` for whole numbers and listed levels. A mutation of an ordered gene is the
  triangular step `u + scale * (r1 + r2 - 1)` reflected at the bounds, a listed-level gene is drawn
  again, and crossover is uniform. No draw goes through libm, and a trajectory is the same on every
  platform. `evaluation_log.rs` (`EvaluationLog`) files every replicate value under its candidate,
  a re-evaluation's under the candidate it repeats, and scores a failed replicate as `Goal::worst`.
  Among equal scores the lower id ranks first. The four searchers are `random.rs`, `hill_climb.rs`
  ((1 + lambda) with lambda the batch size, a restart after `patience` batches without a strict
  improvement, an optional re-evaluation of the incumbent, and starting points drawn through
  `draw_config` as neighbours are, so no two first evaluations share a config), `genetic.rs`
  (generational, tournament selection, uniform crossover, per-gene mutation, children bred as a
  generation starts from the fitness before its re-evaluations, `elite_count` survivors chosen after
  `ceil(reevaluate_fraction * population)` re-evaluations of the best members, fitness pooled over
  every replicate a member has, no batch spanning two generations, and `MAX_POPULATION` and
  `MAX_TOURNAMENT_SIZE` of `1 << 16`) and `pse.rs` (Pattern Space
  Exploration over two `PatternAxis` outputs, with a `BTreeMap` archive keyed by `PatternCell`
  holding the hits and the first candidate to land, and parents taken from a two-way tournament on
  fewest hits after `initial_samples` random candidates). The initial samples fill batches of their
  own, and without any the first candidate is alone in its batch. After them, an ask on an empty
  archive draws a whole random batch. `PatternSpaceSettings::batch_count` gives the number of
  batches, and `PatternAxis::cell_index` puts NaN in cell 0, outside. An axis with neither `min` nor
  `max` has an automatic range: the searcher holds every evaluation until the initial samples are
  told, takes the range from their outputs (`automatic_range`, 5% margin), then files the held ones
  in candidate order. `Searcher::pattern_settings` returns the settings once both axes have a range,
  and `SearchUpdate`, `SearchHistory` and the manifest's `axis_ranges` carry them.
  `SearchHistory::read` takes the range again from the initial samples' rows of `evaluations.csv`
  and places the rows written before it. `search/tests/` holds the driver the searcher tests share
  (`support.rs`) and the protocol tests every searcher passes.
- **henad-build**: a build dependency, on henad-core and `wgsl_bindgen` alone. `lib.rs` holds
  `ShaderBuild` and `ShaderBuildError`, `paths.rs` the walk of a shader root and the Rust names a
  path becomes (`check_components`, `check_reserved`, `check_collisions`), `binding_lines.rs` the
  private reader of `@group(0)` lines, and `output.rs` the shared-module copies, the stamp and the
  two generated files. Its tests sit in `src/tests/`: the path checks, the reader against the
  layout `wgsl_bindgen` generates, and whole generations (`a_second_build_reruns_nothing`,
  `a_shader_added_between_builds_is_generated`, `a_crate_without_shaders_builds`). Keep them.
- **henad-compute**: the engine machinery that turns an authoring impl into something runnable.
  `cpu/` and `gpu/` are **siblings**, not a base and a specialisation, and mirror each other:
  each has its own `sim_thread.rs` (runner), its `*_engine.rs` (authoring trait → runnable state)
  and `primitives/` (shared building blocks). `snapshot.rs`, `runtime_info.rs` and
  `display_scale.rs` sit above both, since either backend publishes through them. So does `runner/`,
  which owns how a sim loop gets driven and the one place the two ways of driving one differ:
  `runner/mod.rs` holds the `SimLoop` trait, `Pace` and the `SnapshotSlot`, with `runner/thread.rs`
  the native driver and `runner/frame.rs` the wasm one. `entry/` type-erases a model for a host:
  `ModelEntry` (declarations behind accessors and an `Arc`, and a factory that takes the device at
  `build`), `ModelState`, the five `register_*` generics, and `ModelSet` (`entry/set.rs`). An entry
  holds no device from its registration. A set refuses a duplicate id or one outside the grammar,
  records its `BuildInfo` on an entry that has none, and merges the entries' `GpuNeeds` for the
  device request. The `register_*` functions are generic, and the engines and kernels monomorphise
  in the crate that calls them. henad-compute instantiates none outside its own tests.
  `wrap_factory` and `Factory` are `#[doc(hidden)]` and public for henad-explore's test harnesses
  alone. `grid_init_rng` and `agent_init_rng` hold the default-seed rule the CPU engines and the GPU
  ports share.
  - `cpu/grid_engine.rs` (`GridModelState`), `cpu/agent_engine.rs` (`AgentModelState`) and
    `cpu/network_engine.rs` (`NetworkModelState`) each implement the whole `SimState` for their
    trait. `cpu/field/ca.rs` (`CaField`, a `GridModel` as
    a field layer) and `cpu/field/scalar.rs` (`ScalarField`, scatter-plus-decay `f32` layers) are
    the two `FieldLayer` impls. `cpu/layout.rs` is the spring layout for a network's node
    positions, NetLogo's `layout-spring` with three changes. An edge's pull levels off as it
    stretches, repulsion is cut off at a radius found through a `SpatialHash`, and a node slows
    down while its force keeps swinging, as in `ForceAtlas2`. Every `SpringParams` constant is in
    units of the mean spacing between nodes. `cpu/primitives/` holds `lanes_macro.rs`
    (`agent_lanes!`, for node lanes too), `chunked.rs` (chunk drivers and RNG seeding), `scatter.rs`
    (the many-agents-one-cell write path) and `components.rs` (connected components by min-label
    propagation). `AgentModelState` counts its model's own params once at construction, and a step
    builds no descriptor list. `cpu/sim_thread.rs` is the sim runner, a `SimLoop` with play/pause/TPS-capping,
    driven by whichever `runner::Driver` the target has.
  - `gpu/grid_engine.rs` (`GpuGridState`) and `gpu/agent_engine.rs` (`GpuAgentState`) are the
    engines for the two GPU traits, mirroring their `cpu/` namesakes. `gpu/sim_thread.rs` is the
    batching GPU runner and `gpu/timing.rs` its adaptive-batch controller. `gpu/view/` is what a
    model hands the UI (`display.rs` for a texture layer, `agents.rs` for lane buffers drawn in
    place). `gpu/primitives/` holds the GPU counterparts of henad-core's data structures —
    `spatial_hash.rs`, `prefix_scan.rs`, `reduce.rs`, `readback.rs` — plus `dispatch.rs` and
    `pipeline.rs`. The `henad::dispatch` module a pass imports and the `reduce_tree` fold a reduce
    leaf repeats are WGSL in henad-core's `authoring/primitives/wgsl/`. `gpu/grid_dims.wgsl` is an
    entry point that is never dispatched. It brings `henad::dims::Dims` into the generated
    bindings, and `grid_engine.rs`'s `Dims` is a `pub(crate)` alias of that struct.
    `gpu/tests/parity.wgsl` is the parity test's shader, with a `CODES` array that references
    each boundary and table code the test reads.
    `gpu/limits.rs` is what raises the device past the WebGPU baseline, and `gpu/capacity.rs`
    is what asks whether a model fits the device before anything is allocated.
    `gpu/stepping.rs` steps a `GpuSimState` from a host with no sim thread, native only. It submits
    at most `MAX_STEPS_PER_SUBMISSION` steps per command buffer, fires a `Schedule` under either
    `Fire` rule, waits once at the end of a run, and blocks on a stats readback. `submit_slice`
    submits one command buffer of steps, with the stats passes when sampling, and returns its
    `SubmissionIndex` without waiting. Its stats passes are `GpuSimState::encode_stats_passes`, the
    snapshot passes without the display pass. `await_submission` waits for one submission and those
    before it.
    A shared file name always means _counterpart_, never coincidence: `cpu/sim_thread.rs` and
    `gpu/sim_thread.rs`, `cpu/agent_engine.rs` and `gpu/agent_engine.rs`, `ants/step.rs`,
    `boids/step.rs` and `virus_network/step.rs`, `henad-core/src/spatial_hash.rs` and its
    GPU twin. Two unrelated things must not share a basename.
    The one name carrying three meanings is `primitives`, so keep them straight: `cpu/primitives/`
    and `gpu/primitives/` are engine internals and are counterparts of each other, while
    `henad-core/src/authoring/primitives/` is the model-author-facing vocabulary and is a
    counterpart of its own `wgsl/` directory instead.

- **henad-models**: concrete simulations — `sir.rs` and `game_of_life.rs` (`GridModel`), `boids/`
  (`AgentModel` over `NoField`), `ants/` (`AgentModel` over `ScalarField`, the one composite
  model), `virus_network/` and `team_assembly/` (`NetworkModel`), `gpu_game_of_life/` and
  `gpu_sir/` (`GpuGridModel`), `gpu_boids/` and `gpu_ants/`
  (`GpuAgentModel`). A CPU agent model is split into
  `lanes.rs` (the `agent_lanes!` declaration), `mod.rs` (metadata, params, stats) and `step.rs`
  (the kernels); ants adds `field.rs` for its pheromone layer. A network model has the same
  `lanes.rs` and `mod.rs`. `virus_network/` adds `step.rs` (the node pass) and `wiring.rs` (the
  initial edges and the rewire). `team_assembly/` has no node pass and no `step.rs`. It adds
  `assembly.rs` (the global pass and the setup teams), `ring.rs` (`RetirementRing`, a bucket queue
  that finds the nodes due to retire without a scan) and `live.rs` (`LiveSet`, a dense list of the
  live nodes for uniform draws). A GPU model is one `mod.rs` of
  declarations next to its `.wgsl` files. Each GPU port seeds itself through its CPU counterpart's
  `init`, which is what keeps tick 0 bit identical between the two backends and makes them fair to
  compare — that call is confined to `seed_buffers`. `example_models()` (`lib.rs`) registers all
  ten into one `ModelSet` with henad-models' own `build_info!()`. The registry tests sit in
  `src/tests/registry.rs` and run over `example_models()`. `gpu_boids` declares
  `REPLAYS_EXACTLY = false`, and its entry's `metadata().replays_exactly` reads it.
- **henad-explore**: sweeps and searches, a sibling of henad-models over henad-compute, below the
  two front ends. `sweep.rs` (`run_sweep`) plans a `SweepSpec`, checks every config of a GPU model
  against the device (`probe.rs`, `check_capacity`), and builds the first config without a fault
  (`ProbeReport`) to fix the stat columns and bind the reducers and the stop condition. It builds
  the last config as well, and the larger of the two sizes the lanes or tracks and the projected
  memory. A config that faults is left for its runs to record. A scatter grid sizes its scratch to
  the pool. Lanes narrower than the global pool get the probe rebuilt on a pool of their width, and
  the memory cap reads that build. It then writes the manifest, runs every pending run, rebuilds the
  summary and replaces the manifest. A sweep that fails after the manifest exists marks it `failed`
  when it can. A dry run stops after the probes. `spec_file.rs` is the TOML form of a spec
  (`SpecFile`). Every table is `deny_unknown_fields`. A param value stays the text `--set` takes,
  and a spec file and a command line hand `parse_value` the same text. A `table` block's `file` is
  read relative to the spec file, and only a table design's. A path that is absolute or holds `.` or
  `..` is refused (`SpecFileError::TablePath`). The manifest's copy of the spec carries the table
  inline as `table_text`, and the writer emits `design_seed` for a sampled design alone. `specs/`
  holds real spec files and a design table. The docs include each spec by `--8<--` region and the
  table whole, and a test plans the specs. `schema.rs` (`schema_json`) is the `--params --json`
  object, also embedded in the manifest. `cursor.rs` (`RunCursor`) owns one CPU run from its build
  to its `RunOutcome`. `advance(max_steps)` steps it a slice at a time and takes every sample due on
  the way, `prepare_view` then `stats()`. It fires tick 0's actions before the first step and each
  later tick's after the step that reaches it (`Fire::AfterStep`), through `Schedule::run_due`. The
  stepper tests for an empty schedule once per slice, and a run with no actions steps with nothing
  else in its loop. A refused action goes in the note and leaves the status alone. The run ends on
  the first sample where its stop condition holds, or after the first slice past its timeout
  (`timed_out`, checked by wall clock between slices). A fault ends the run with its status, and the
  samples before it are kept. A cursor refuses a GPU model. A GPU run steps on a track of
  `exec/gpu.rs` under the same rules, and both end a run through `run_outcome`. `exec/mod.rs` holds
  `Concurrency`, `choose_layout` (the auto rule), `ExecutionLayout`, `GpuTrackDepth`, `SweepControl`
  (pause and abort, one atomic load per slice), `Executor` and the reorder buffer that commits
  outcomes to a `RunSink` in request order. `choose_layout` sizes CPU lanes from the probe's
  `parallel_jobs`, and GPU tracks as `gpu_memory_budget` over the probe's `Demand`, at most
  `MAX_AUTO_GPU_TRACKS`, with a single track from a population of `LARGE_GPU_POPULATION`. Each run
  starts its slices at one step. A slice carried over from a lighter run once held off a pause for
  minutes. `exec/cpu.rs` runs one `thread::scope` lane per rayon pool, each pool built once in
  `Executor::new` with `threads_per_lane` workers, and each run built and stepped inside
  `pool.install`. A single lane runs inside `rayon::scope` on the global pool, as `bench_cpu` does.
  A panic on a lane's thread or in the sink aborts the `SweepControl` (`AbortOnPanic`). The other
  lanes then stop within a slice instead of stepping runs nobody commits. `exec/gpu.rs`
  (`run_on_tracks`) is the counterpart of `exec/cpu.rs` for a GPU model. Error scopes are
  thread-local, and every `GpuTrack` lives on the calling thread. The `Interleaver` builds a queued
  run once a track is free and the summed `entry.demand()` bytes of the live runs and its own fit
  the budget (`--gpu-memory`, else the device's `max_buffer_size`). A run that fits beside none runs
  alone. An out-of-memory build beside live runs goes back to the head of the queue, drops the track
  cap by one and holds admission until a live run ends. Each round reads the `SweepControl` once and
  visits every track once. A visit takes a landed sample through `poll_stats_readback(false)`,
  submits the actions due at the track's tick, each on its own, then one `stepping::submit_slice` of
  at most `steps_per_submission` steps, cut at the next tick a sample or an action is due at. A
  track keeps at most `submissions_per_track` buffers on the device, and records the interval after
  a sample while that sample reads back. The sample's tick is kept at encode time. By the time the
  readback lands, `state.tick()` has moved on. A round in which no track moved waits for the oldest
  submission through `stepping::await_submission`. A blocking readback poll would drain the whole
  device and stall every track. Each action and slice runs inside `catching_on`, and its fault ends
  its own track. A fault left in `ctx.faults` ends every live track, and a run that ended earlier in
  the round keeps its end. A run ends at its last landed sample, and the steps recorded past a
  sample that stops it are dropped with the state. Each round's time is split evenly between the
  live tracks, and a track's clock counts its share, builds and pauses aside. The timeout, `wall_ms`
  and `steps_per_s` read that clock. A lost device ends the batch as `BatchEnd::DeviceLost`, the
  manifest reads `incomplete`, and `--resume` runs the rest. `output/`
  writes `runs.csv` (an `action.<name>` column per action after the params), `series.csv`,
  `summary.csv` (rebuilt at the end of a sweep, a resume or a merge by `write_summary`, which
  streams `runs.csv` one record at a time and holds one cell string per config) and
  `manifest.json` (written `running`, then replaced by a rename). `OutputDir` refuses a directory
  that holds any of the four, a search table or any `*.staged` file unless the sweep resumes, and
  `OutputWriter` writes and flushes a run's series before its row. `output/read.rs` reads the tables
  back, keeping complete records only (`RunsCsv`, `SeriesScan`), and `merge_series` interleaves
  series segments by run id. `record_ends` follows the CSV grammar (`RecordScan`), and a quote
  inside an unquoted field fails its record instead of hiding the records after it.
  `output/resume.rs` (`ResumeScan`) accepts a directory only with the same plan hash, schema hash
  and shard, and no more replicates than the sweep runs. It keeps `ok`, `non_finite` and, without
  `--retry-failed`, failed runs, always reruns a `timed_out` one, renumbers kept runs when the
  replicate count rises, and repairs the tables by truncation or by a rewrite. A rewrite stages both
  tables as `*.staged`, marks them complete with `tables.staged`, then renames both, and
  `OutputDir::open` finishes a rename a process left part done. `merge.rs` joins shard directories
  of one plan, replicate count and shard count, merges both tables by run id, rebuilds the summary
  and records the inputs in `merged_shards`. Missing runs are a warning and an `incomplete`
  manifest. A resume of the merged directory fills them in. `progress.rs` is the `Progress` trait
  the host renders. The library never prints. `device.rs` (`acquire_headless`) acquires a GPU device
  with no window or surface, at the WebGPU baseline raised by `gpu::limits::raise`. An adapter
  below the baseline, as a GL adapter can be, gets `DeviceError::BelowBaseline`, and the CLI then
  runs CPU models only. It is native only. `pollster` blocks on the request, and a browser cannot
  block. The crate is in the wasm typecheck with henad-core, henad-compute and henad-models.
  Native-only code sits behind `#[cfg(not(target_arch = "wasm32"))]`. `handle.rs` (`SweepRun`) is a
  host's handle on a sweep, with one API on native and in a browser, described under "Sim runs off
  the UI thread". `SweepRun::start` plans the spec before it returns, and a browser refuses a GPU
  model with `SweepStartError::GpuNeedsNative`. The `gpu` a host passes to `start` or
  `resume_directory` is a device it shares with the sweep, `FaultSink` included. Handed none for a
  GPU model, the sweep thread acquires a device sized to the entry's `GpuNeeds` through
  `acquire_headless`, builds the entry it was handed on it (`BoundModel`), and records that device
  in the manifest. A device it cannot acquire fails the sweep with `SweepEvent::Failed`.
  `acquire_headless` returns the `GpuContext` alone, with its `RuntimeInfo` attached and read back
  through `runtime_info()`.
  `SweepOutput::Memory` writes the four files through the same writers over `Vec<u8>`
  (`output/memory.rs`, `SweepFiles`) and hands them over in the `SweepRecord`.
  `SweepOutput::Directory` is native only. `SweepRunOptions::memory_budget` and `gpu_memory` are
  the budgets of `--memory` and `--gpu-memory`. `SweepRunOptions::series_budget` caps the bytes of
  series the `RunFinished` events carry. From the first run past it, every run arrives without its
  series, and the files keep them all. `SweepRun::resume_directory` resumes a folder with the spec
  its manifest records, along the path `--resume` takes. `pumped.rs` (`PumpedSweep`) is the
  browser's executor, a `runner::SimLoop` that makes one probe build per pump (`PlanProbe`),
  prepares the sweep in a pump of its own, then builds and steps one CPU `RunCursor` at a time
  into memory, a slice of about half `PUMP_BUDGET_MS` per pump. A pause holds only the runs, and a
  sweep or search paused after its last run still ends. It compiles for wasm32 and for tests, and
  the tests pump it on native. `result_set.rs` (`ResultSet`) reads an output directory back, through
  `open_dir` on native or `from_files` over the bytes of picked files. It needs `manifest.json` and
  `runs.csv`, reads complete records only, and holds `series.csv` run by run in file order while a
  byte budget lasts. A run the budget cuts holds none of its rows. A `runs.csv` with no complete
  header, as a sweep stopped before its first run leaves it, opens with no runs and no value or
  reducer columns. `series.csv`'s header has to name `run_id`, `tick` and the manifest's stat
  columns. `summary.csv` is left unread. `ResultSet::replay` plans the recorded spec and returns a
  run's `Replay`, refusing a run whose config, replicate, seed or run key differ from its row. The
  run key hashes the model's declarations too, and is compared only while `schema_matches` holds.
  `read_directory_series` reads the series of chosen runs later, given the stat column names.
  `src/tests/` holds `support.rs` (the headless device, scratch directories, a sweep helper,
  `OutputTables`, `CommitLimit`, and `ticks_seen` with `HOLD_WINDOW` for pause checks), `broken.rs`
  (`GridModel`s that panic or report a value that is not finite, registered through the public
  `register_grid_model`), and the determinism, failure, run control, resume, shard, GPU sweep, GPU
  track (`tracks.rs`), handle, result set and replay tests. For a model that replays exactly (every
  model but `gpu_boids`), the three CSVs are byte-identical apart from `TIMING_COLUMNS` at any lane
  or track count, for merged shards against an unsharded sweep, and for a resumed sweep against a
  fresh one, timed-out runs aside. A sweep held in memory, on a thread or pumped, writes the bytes a
  directory sweep writes. `OutputTables` compares `series.csv` and `summary.csv` byte for byte, and
  `runs.csv` with its timing fields emptied.
  `a_sweep_writes_the_same_files_at_any_concurrency`, `interleaved_gpu_runs_match_sequential_ones`,
  `ants_results_do_not_depend_on_lane_width`, `merged_shards_equal_an_unsharded_sweep`,
  `a_resumed_sweep_skips_finished_runs_and_matches_a_fresh_one`,
  `memory_output_equals_directory_output` and
  `the_pumped_sweep_writes_what_a_directory_sweep_writes` hold that line.
  `a_pipelined_gpu_sample_matches_a_blocking_one` pins a track's pipelined sample to
  `stepping::sample_stats`. `gpu_boids` matches only in the parts the engine owns, every `runs.csv`
  field up to `population` and the run and tick of each series row, and
  `interleaved_gpu_boids_runs_commit_in_plan_order` pins them. Keep them.
  `search_run.rs` (`run_search`) runs a spec with a `[search]` table. `SearchPlan` plans the fixed
  values and actions as a one-config `Plan`, and a search spec with blocks is refused. It resolves
  the space, checks that every watched column is a reducer column, and hashes the settings that fix
  the trajectory with `search_hash`. Unlike `plan_hash`, that hash covers the replicate count, and
  it covers the budget. The loop asks the searcher for a batch, runs it through the sweep's
  `Executor::run_batch` on lanes or tracks, and tells the searcher the watched reducer values. Run
  `r` of candidate `c` has run id `c * replicates + r`, config id `c`, and replicate index
  `replicate_offset + r`, seeded by the spec's scheme. Common random numbers then hold across
  candidates, and a re-evaluation runs on fresh seeds. `SearchSession::tell` turns each batch into
  a `SearchUpdate`, reported as `ProgressEvent::SearchBatchTold`. `output/search_tables.rs` writes
  `evaluations.csv`, `batches.csv` and, for a genetic algorithm, `generations.csv` batch by batch,
  then `best.csv` or `archive.csv` at the end, and `SearchHistory` reads them back for the app. It
  finds the columns after a config by their place from the end of the header, since a param id can
  share their names, tells a PSE table by its last column, and checks the trailing cells of a
  scored `evaluations.csv`. A
  resume replays the searcher from its seed and reads every recorded run's values back from
  `runs.csv` (`RecordedSearch`) in place of running it, failed and timed-out runs included. It
  refuses a run whose `run_key` differs from the run asked for, and replays every recorded batch
  once in `SearchPreparation::new` to check them all before a table is written. A search refuses
  `--shard` and `--retry-failed`. The manifest reads `mode` `search`, holds the hash of the fixed
  values and actions as `plan_hash` and `null` as `plan.configs`, and adds a `ManifestSearch` with
  the search hash. A search that fails records its standing at the failure there. `SweepRun` and
  `PumpedSweep` run searches as well, and `ResultSet` reads
  a search folder back with its tables. `specs/sir_search_genetic.toml` and
  `specs/sir_search_pse.toml` are the example searches the docs include. `tests/search.rs` holds
  `every_example_search_spec_parses`, `a_search_writes_the_same_tables_at_any_concurrency`,
  `a_resumed_search_follows_the_same_trajectory`,
  `a_resume_that_meets_a_changed_run_leaves_the_directory_alone` and
  `a_gpu_search_writes_the_same_tables_on_any_track_count`. Keep them.
- **henad-app**: eframe/egui desktop+web GUI. `HenadApp` (`lib.rs`) owns the `SimThread` and
  polls snapshots each frame. `HenadApp::new` takes the `ModelSet` the host offers, and
  `wgpu_configuration` takes the set's `GpuNeeds` for the device request. `main.rs` passes
  `example_models()` to both; `ui/` has one file per panel or window (`menu_bar.rs`, `model.rs`,
  `params.rs`, `playback.rs`, `pacing.rs`, `viewport.rs`, `stats.rs`, `charts.rs`,
  `performance.rs`, `system.rs`, `fault.rs`, `about.rs`). The Export tab lives in a directory,
  `export/`. Its `mod.rs` draws the tab and writes the stat series and final state, `image.rs`
  captures the viewport and `metadata.rs` builds the run details. `files/` holds the file dialogs:
  `save.rs` hands bytes to a save dialog (several files go into one picked folder on native and
  download one by one in a browser), and `open.rs` reads picked files, or takes a picked folder on
  native. Each outcome carries a `SaveTarget` or `OpenTarget`, and `AppState::poll_saves` and
  `poll_opens` route it to the panel that asked. `dock.rs` holds the tab definitions, the default
  layout and the dispatch to each panel. Sweep and Results stack behind Viewport.
  `agent_layer.rs` (with `agents.wgsl`) is the instanced agent renderer drawn over the grid layer,
  and `painted.rs` carries a paint callback's wgpu handles past `CallbackTrait`'s `Send + Sync`
  bound. `edge_layer.rs` (with `edges.wgsl`) draws a network's
  edges under the nodes. It reads both ends from the agent layer's position buffers, bound as
  storage, and uploads the edge list only when its version or length changes. That binding
  needs `DownlevelFlags::VERTEX_STORAGE`, and WebGL2 lacks it. Without that flag there is no edge
  layer, and a network model runs with its edges undrawn. `world.wgsl` holds the helpers both
  shaders `#import`, including `is_placed`, the finiteness test that hides a retired node.
  `state.rs` (`AppState`) holds the set as `models` and keys the selection by id:
  `selected_model` and `loaded_model` are `Option<String>`, resolved at use through
  `selected_entry`, `loaded_entry` and `lookup`. `offered_models` hides a GPU model where the
  adapter has no compute, through `ModelSet::runnable`, and the app opens on the first offered
  model, or with none selected and `NO_MODEL_RUNS` in the Model panel. A missing id reads as
  `lookup_message` words it, "This build does not include model 'x'" or "Model 'x' needs a GPU with
  compute support, and this device has none", with no full stop. `select_model` loads the defaults
  and clears the schedule, for the Model panel and the Sweep tab alike, and an id the set lacks
  leaves no values. `AppState` also holds the values the next build reads: `param_values`, the Seed
  field's `seed` with its raw `seed_text`, and the scheduled actions in `schedule`. `build_runner`
  passes `seed` to the entry's factory, and `reset_simulation` sends `SimCommand::SetSchedule` to
  the new runner when the schedule is not empty, then records `loaded_seed` and `loaded_schedule`.
  `seed_pending` and `schedule_pending` compare each with its loaded copy, and the Reload needed
  notice counts both. A `None` seed is the model's default, the constant `henad-cli` uses without
  `--seed`. The engines take `seed.map_or(CONST, mix_seed)`, and the field keeps `None` as a state
  of its own, shown as the "Default" hint and never as a number. `params.rs` draws the Seed row
  above the sliders, its dice (`draw_seed`, the clock mixed through `mix_seed`), and the
  scheduled-actions list under the action buttons. `parse_seed` reads an empty field as `None` and
  a decimal `u64` as `Some`. Anything else shows `INVALID_SEED` and disables Build. Picking another
  model clears the schedule. Its entries index the previous model's actions. `playback.rs` draws
  the Run to tick row. `AppState::run_to` rebuilds first for a tick behind the current one, then
  sends `SimCommand::RunTo` and sets `run_to_target`. Run is disabled while another model is
  selected, while the sim plays, and for a tick behind the current one while Build is disabled.
  A playing loop runs ahead of the snapshot's tick, and a target between the two would pause past
  it. The progress line reads the target, Cancel sends `Pause`, and `HenadApp::logic` clears the
  target once a snapshot reaches it. A snapshot past the target makes `logic` call `run_to` again,
  and the second call rebuilds. Where the frame drives the sim (`!CAN_SPAWN_THREADS`), `logic`
  keeps repainting while a run-to is pending. `AppState::open_run(Replay, OpenAt)` builds one sweep
  run from a `henad_core::explore::replay::Replay`. It selects the model, sets the params, seed
  and schedule, rebuilds, sends `RunTo` for `OpenAt::Tick`, and sets `focus_request` to the
  Viewport. `HenadApp::ui` applies that request through `dock::focus_tab` after
  `DockArea::show_inside`. The panels draw while the dock is borrowed. `OpenedRun` names the run in
  Playback. A live param edit, an action press or a build from other values marks it modified
  (`settle_opened_run`), and a build of another model or Offload drops it. `export/metadata.rs`
  writes the loaded `seed` (null for the default) and `scheduled_actions`.
  `ui/sweep/` is the Sweep tab. `mod.rs` holds `SweepPanel` and `sweep_ui`, which splits the tab
  into four egui panels: a header and a footer of fixed height (`header.rs`, `footer.rs`), a Plan
  panel on the right while the tab is wide enough (`PLAN_PANEL_BREAKPOINT`), and a central scroll
  area holding the form or the progress. The header and footer keep their ids and heights in every
  state, and the last slot of the footer holds Start, then Pause or Resume, then Show results. A
  press or a link sends a `SweepRequest`, applied once every panel has drawn. `dock.rs` turns off
  `egui_dock`'s scroll bars for the tab and titles it with the session's state, as in "Sweep 42%".
  `draft.rs` is pure and unit-tested: a `SweepDraft` per model holds the builder's fields as typed,
  level text read by `LevelSpec::parse`, and converts to a `SweepSpec` (the Parameters panel values
  fill every parameter it does not vary) and back, TOML through
  `henad_explore::spec_file::SpecFile`. "One at a time" writes one factorial block per varied
  parameter or action tick, each other varied parameter pinned to its panel value as a one-value
  factor, since a spec cannot fix a parameter that any block varies. `from_spec` recognises that
  shape and refuses other multi-block specs. `SweepDraft::check` collects every issue its pre-pass
  finds before it plans, each a `DraftIssue` with the `DraftSite` of its row and an `IssueKind`:
  `Missing` for input not given yet, `Invalid` for input the plan refuses. Both block Start.
  `SweepPanel::cached_check` plans the draft only when it or the panel values change, and caps a
  sweep at `MAX_DRAFT_RUNS`. The panel learns each model's stat columns from one build at its
  default values (`ColumnsBuild` in `mod.rs`), on a thread named `henad-stat-columns` whose report
  wakes the UI, a GPU model on a headless device of its own. A browser builds a CPU model inline
  and skips a GPU model. `SweepDraft::set_stat_columns` hands the columns to the draft, which then
  names outputs as `ReducerPlan::bind` does (a vector's `.x`, `.y` and `.magnitude`, a histogram's
  `.total`), turns a watched bare vector label into its magnitude column, and in `check` binds the
  reducers, the stop condition and a search's watched columns through `MeasurePlan::new` and
  `SearchPlan::watched_reducers`. While the build runs, `columns_pending` holds the Missing issue
  `COLUMNS_PENDING` ("Reading model outputs") at `DraftSite::Outputs` and Start stays disabled.
  After a failed build each stat counts as one column, and a watched `Label.part:kind` of a
  default kind is accepted for the sweep's own probe to check.
  `row_levels` caps at `MAX_DRAFT_LEVELS` only a range the draft lists, and a range with no step
  under a sampled design or a search can be any size. `SweepDraft::tick_source` (`TickSource`)
  gives an action's tick as fixed, varied or taken from the design table's `action.<name>` column.
  Save spec (`to_toml`) also refuses what the loader would, a reversed window or a threshold that is
  not finite, and reads its own TOML back as a check. The draft keeps a loaded spec's `[execution]`
  `memory` and `gpu_memory` (`memory_budget`, `gpu_memory_budget`). Save spec writes them back,
  Start passes them to `SweepRunOptions` through `SessionExecution`, the Plan lists them, and the
  Execution section shows them read-only beside a clear button. `MIN_TIMEOUT_SECONDS` bounds only
  the Timeout field, and a loaded shorter timeout, 0 included, is kept, as `henad-cli` takes it.
  `CheckSummary` turns the check into the lines the footer, its chips and the Plan panel list.
  `plan.rs` (`PlanSummary`) is the plan the panel, the narrow Plan section
  and a session draw. `layout.rs` holds `FormLayout`, the label column every section shares,
  `SIDE_BY_SIDE_BREAKPOINT`, the section headers with their issue counts, the note line a row
  reserves before any note comes, the `Reveal` that opens a section and scrolls to a row, and
  `ISSUE_DELAY`, the time a focused text field holds back the issue of its text. `builder.rs`
  draws the Design, Actions, Replicates and seeds, Run length, Outputs and Execution sections, and
  `parameters.rs` the parameter rows, the values editor and the options menu, each writing the
  row's `--vary` text, a list without spaces. `session.rs` wraps `henad_explore::handle::SweepRun`
  (a thread on native, pumped from `HenadApp::logic` through `ui::sweep::update` in a browser, where
  the frame keeps repainting while it steps), pauses the live sim on start, and keeps the
  `KeptDraft` that started it for Save spec and the Plan panel. It hands `SweepRun` no device, so a
  GPU sweep steps on a device of its own and never shares the app's `FaultSink`. A fault in
  `render_ctx.faults` offloads the live model and leaves a running sweep alone, and `SessionState`
  has no fault pause.
  `progress.rs` draws the Status or Result grid, the Search grid and Runs in progress, and
  `footer.rs` Pause, Resume, Abort with its modal, and the end with
  Edit sweep, Save results and Show results. Show results sets `focus_request` to Results, as Open
  on a run sets it to Viewport. Show failed runs and Show in Results go through
  `ResultsPanel::show_failed_runs` and `ResultsPanel::select_candidate`. Events go to
  `ui/results/`, the Results tab, whose `ResultsPanel` also keeps the in-memory files of the last
  sweep for Save results. `store.rs` (`ResultsStore`) holds every run of the sweep, or of a folder
  read through `henad_explore::result_set::ResultSet`, the configs with their axes (the value
  columns that take more than one value), and a `SeriesCache` of series within
  `DEFAULT_SERIES_BUDGET` bytes. It computes the Series view's replicate bands (`SeriesBand`), the
  Response view's points and the heatmap's cells, each pooling a config's runs that did not fail
  in order of run id, and the runs table's order. `summary.csv` pools in `runs.csv` order, which a
  resume can leave out of run-id order, and the last digits can then differ. The other views cache
  on `ResultsStore::revision`, and the Series view on the drawn configs' counts of runs and held
  series plus `ResultsStore::replaced_count`, the runs a resumed sweep's rerun replaced.
  `series.rs` draws a `FilledArea` band and a centre line per config, `response.rs` caches each
  line as a `PlottedLine` and draws its whiskers as one `Whiskers` item with the line's id, so the
  legend hides both, and `heatmap.rs` draws the grid. `plot.rs` holds the palette, `decimate` (the
  lowest and highest point of each bucket), the heatmap scale and `HeatmapTiles`, the plot item the
  Heatmap view and the PSE grid draw through. It borrows the values, and a cell without a value, or
  an empty PSE cell, draws as `NO_DATA_COLOR`. `table.rs` draws the runs with
  `egui_extras::TableBuilder`, and the detail strip whose Open and Open at end go through
  `AppState::open_run`. Open at end steps to the run's recorded ticks, which a stop condition can
  bring early. The store refuses to replay a run whose plan gives another run key than its row,
  while the model's schema matches the sweep's. After a model change the replay opens under the
  table's warning. Copy command puts an equivalent `henad-cli --export-stats` line on the
  clipboard, which samples from tick 0 on the CLI's own cadence. Open results reads a folder on a
  thread of its own on native and the picked files in a browser, on the frame after the one that
  first shows "Reading results" (`hold_picked_files`, `due_picked_files`). `ui::results::poll` takes
  the `egui::Context` for it.
  Resume sweep resumes an incomplete folder through `SweepRun::resume_directory`, and the folder is
  read again once that sweep ends. `henad-app --open DIR` opens a folder at start.
  `ui/sweep/search.rs` draws the Search section of Search mode: the method, the objective or the
  PSE axes, the budget, and each method's other settings in a nested Method settings section. An
  output picked there that the runs do not record joins the Outputs section, and Use range from
  results reads `ResultsStore::output_range`. `SearchDraft` (`draft.rs`) keeps the settings of
  every method across a switch. `ui/results/search.rs` is the Search view: the best so
  far against evaluations for random search and hill climbing, the best, median and worst of each
  generation for the genetic algorithm, and the PSE grid, where a click selects the first run of a
  cell's exemplar. `ResultsPanel::ingest` takes a frame's events together, as `SweepSession::update`
  drains them, and `ResultsStore::push_search_updates` records each `SearchBatchTold` batch by
  batch, then adds the frame's new configs in one pass. The pass extends the configs and axes in
  place (`fits_axes`, `extend_configs`), and a level keeps its id when levels are added below it.
  Every config is built again only when a batch brings a second value to a column that is not an
  axis, a text that is not a finite number to a numeric axis, or a candidate id at or below one
  held. Runs that land before their candidate is told wait in `unassigned_runs`. A new level on a
  numeric axis moves every level above it, and an `f32` search costs time linear in its levels per
  frame.
- **henad-cli**: headless benchmark runner. Steps a state in a bare loop with no rendering, no
  `SimThread` and no pacing, so a measurement times nothing but `step()`. `--act ID@TICK`
  (`henad_core::action::Schedule`) runs a declared action before the step at that tick. An action
  due from the end of warm-up up to, but not including, the tick the run stops on is timed with the
  steps, and one due on that tick runs after the timer stops. The GPU path fires the same ticks.
  The GPU benchmark fires before the step and the GPU stats export after it, one `action::Fire`
  rule per run, as the two CPU loops do. Otherwise two runs back to back both fire the tick they
  share. `actions.rs` holds the benchmark's rule, `BENCH_FIRE`, and prints the actions a model
  refuses. A GPU run steps through `gpu/stepping.rs` on a device from henad-explore's
  `acquire_headless`, sized to `example_models().gpu_needs()`. The CLI resolves its positional id
  through `ModelSet::lookup`, and refuses an id outside the set ("this build does not include
  model 'x' (try --list)") apart from a GPU model on a machine without a device. `--list` prints
  only the models this machine can run. The CLI never publishes and never lays out a network.
  `--export` calls `prepare_view` before it writes, and `--export-stats` before each sample, as a
  publish would.
  `explore.rs` is the sweep mode. `--out`, `--spec` or `--dry-run` selects it (`Mode::Explore`), and
  no command line from before sweeps changes meaning. `--list` refuses all three, and `--info`
  beside them prints the provenance header and runs the sweep. It turns the flags in `ExploreArgs`
  into a one-block `SweepSpec`, a factorial over every `--vary`, a zip, a `--sample lhs:N` or
  `random:N` draw, or a `--design` table, or loads `--spec`. In a sweep, `--act` adds an action
  named by its id, or by the first of `ID_2`, `ID_3` and so on that no earlier action took, and
  `--vary action.NAME=LEVELS` varies its tick. `--spec` conflicts with every flag that changes a
  result. `--concurrent` (the lanes of a
  CPU model or the tracks of a GPU model), `--memory`, `--gpu-memory`, `--shard`, `--resume` and
  `--retry-failed` pass through to `SweepOptions`, and `--merge DIR... --out DIR` (`Mode::Merge`)
  calls `merge` with no model and no device. It renders `ProgressEvent`s as a text line on stderr
  or as `explore_*` JSON lines, and exits 3 (`SOME_RUNS_NOT_OK`) when a sweep ran to its end with a
  run not `ok`, or a merge lacks a run. A sweep seeds its runs with `run_seed`, and the benchmark
  keeps the `base + i` that `benchmarks/protocol.md` fixes.
  A spec with a `[search]` table goes to `run_search` in place of `run_sweep`. Its plan lists the
  batch size, the objective or the axes, the space and the search seed, and `--json` adds an
  `explore_search_batch` line per batch.
  `--params --json` (`json_report::params`) prints `schema_json` with a `kind` of `params`.
  `scripts/bench_matrix.py` parses the text `--params` prints, and that text is unchanged.
  `tests/golden.rs` compares `--list`, `--params` and `--params --json` byte for byte with what
  0.2.0 printed, recorded in `tests/golden/` by the procedure in its `README.md`. Keep them.
  `build.rs` stamps `HENAD_COMMIT` for the manifest, as henad-app's does.

### Adding a new model

Pick the trait matching the topology and the backend. All five are const metadata plus pure
functions. The engine owns allocation, buffering, chunking, RNG seeding, param storage, the views,
and the whole `SimState` impl.

1. **`GridModel`** (`henad-core/src/authoring/model/grid_model.rs`) — cellular automata over `u8` cells.
   Implement `init`, `step_cell`, `stats` and the consts; `cpu/grid_engine.rs` does the rest, including the parallel
   row-wise step. Grid width/height are prepended to the param list at indices 0 and 1. See
   `game_of_life.rs`, `sir.rs`.
2. **`AgentModel`** (`henad-core/src/authoring/model/agent_model.rs`) — a population of agents, optionally over a
   field. Declare lanes with `agent_lanes!`, then implement `init`, `run_step_pass` and `stats`.
   `run_step_pass` is normally one call to the generated `lanes.run_pass(CHUNK, seed, tick, ..)`
   with a per-agent closure, which is where the chunking and seeding happen — see
   `boids/step.rs::run`. `num_agents`, `world_width` and `world_height` are prepended at indices 0,
   1 and 2. `Lanes` comes from the macro; four more associated types pick the behaviour:
   - `Field` — `NoField` (boids), `ScalarField<S>` for scatter-plus-decay layers (ants), or
     `CaField<M>` to put a `GridModel` underneath a population.
   - `Index` — `SpatialHash` when agents read each other, `NoIndex` when they don't.
   - `Tally` — a per-chunk reduction merged in chunk order, `()` when there's nothing to count.
   - `Params` — hot params extracted once per tick, so the kernel does no enum matching.

   A model needing a second pass over agents before the step (ants filling deposit lanes)
   overrides `run_deposit_pass`.

3. **`GpuGridModel`** (`henad-core/src/authoring/model/gpu_grid_model.rs`) — a grid stepped by a compute
   shader. Three WGSL sources (step, display, reduce), buffer lengths, seeds and a uniform block.
   All `K` buffers ping-pong together. Each pass binds buffers by label, and the shipped display
   and reduce shaders read only the first.
4. **`GpuAgentModel`** (`henad-core/src/authoring/model/gpu_agent_model.rs`) — a population stepped by
   compute shaders. Unlike a grid, a step is a _list_ of passes, because the two real models
   disagree about almost everything structural: boids rebuilds a neighbour index and runs one pass
   over three ping-ponged lanes, ants runs two passes over seven in-place buffers with a display
   pass and a persistent counter. So a model declares `BUFFERS`, `STEP_PASSES` and an optional
   `DISPLAY`, and each pass points at its shader's generated declarations,
   `crate::binding_decls::bindings::<SHADER>`, brought in by `include_shaders!`. The engine builds a
   second buffer side only when some `BufferSpec` asks for it, so a model that writes in place pays
   nothing for double buffering. `Domain` has exactly three variants because those are the three the
   two models use — do not add speculative ones.
5. **`NetworkModel`** (`henad-core/src/authoring/model/network_model.rs`) is a population of nodes
   joined by edges, on the CPU only. Declare node lanes with `agent_lanes!` as for an agent model,
   then implement `init`, `stats` and whichever passes the model needs. `init` gets a graph holding
   every node and no edges. A tick runs the sequential `run_global_pass` first, the only pass of
   the tick that can change the graph. The parallel `run_node_pass` follows, over every slot,
   retired ones included. The node pass is normally one `lanes.run_pass(..)` call, as in
   `virus_network/step.rs::run`, and Team Assembly has none. `num_agents` (the node count),
   `world_width` and `world_height` are prepended at indices 0, 1 and 2. `Params` are hot params
   as for an agent model, and `Aux` is the model's own state outside the lanes and the graph.
   Nodes come and go through `Nodes::spawn` and `Nodes::retire`. Both keep the lanes and the graph
   the same length. A retired node sits at `NaN`, and a reused slot keeps the old node's lane
   values. `stats` sees `Aux` immutably. A stat that walks the graph (Team Assembly's components)
   is computed in `prepare_view`, cached in `Aux` against `Network::version()` and the node count
   (a spawn leaves the version alone), and counted by `aux_heap_bytes`. `directed` is read every
   tick, and a flip rebuilds the rows. `EDGE_PALETTE` colours each edge by its colour byte, and
   `LAYOUT` tunes the spring layout.

Every trait can declare one-off actions, drawn as buttons in the Parameters panel and named by
`henad-cli --act ID@TICK`. A CPU trait lists them in `ACTIONS` and runs them in `act`, and
`henad_core::actions!` declares the list and numbers each entry by its position. A GPU trait's
`ACTIONS` lists `GpuGridAction` or `GpuAgentAction` passes instead, and the seed passed to build
its uniform block is fresh on every press. An action runs between ticks (`SimCommand::Act` in the
runner) and draws from its own stream (`action::action_seed`). Otherwise a press would draw the
numbers the next tick would have.

`SimState` is the runner interface, not a sixth authoring path. Implement one of the traits above
rather than `SimState` directly.

Either way, register the new model in `henad-models/src/lib.rs::example_models()` via the
`register_*` generic for its trait, so it's type-erased into a `ModelEntry` and shows up in the UI.
Nothing about an entry should be written by hand. Name, params, stats, actions and
`topology_hint` are all derived from the trait. The registry tests are the safety net that a
model's declared params, topology, actions and stat series match what its state actually does, and
they cover GPU entries too when a device is available.

### Performance-critical paths — read before touching

- **A parallel pass costs a wake-up per worker, and stepping from outside the pool costs an inject
  on top.** `runner/thread.rs` pumps inside `rayon::scope` and `henad-cli` steps inside one, so the
  passes a kernel runs are injected from a worker rather than from a thread that has to be parked
  and woken for each. `ca.rs::rows_per_leaf` is the other half: one row per rayon leaf handed 48
  workers a 64 by 64 grid as 64 jobs of 64 cells, and the floor turns that into one job. Measured
  together at 13x on the smallest grid rungs.
- `henad-compute/src/cpu/field/ca.rs::step_row_moore`/`step_row_vn` and
  `henad-models/src/*/step.rs` (the agent and node kernels) are the hot inner loops. The x-wrap is
  peeled off both row loops so the interior runs without a per-cell modulo; keep that shape,
  including the `enumerate()` interior loop.
- **rayon runs on every target, web included, and no kernel has a sequential twin.** There are no
  paired `_parallel`/`_sequential` functions to keep in step, and reintroducing one is a regression.
  A `#[cfg(target_arch = "wasm32")]` around a hot loop means someone rebuilt a twin.
- **`for_each_chunk_mut!` is a macro, not a function, and must stay one.** As a generic fn taking
  `F: Fn(..)` the extra closure layer stopped the kernel inlining through it and cost 48% on SIR;
  `#[inline]` did not help. Same trap applies to any new hot-loop driver.
- Determinism: a chunk's RNG comes from `chunk_seed(base, tick, chunk_index)`, never from anything
  a worker mutates, so results don't depend on how rayon schedules chunks. This now has to hold on
  the web too, where the pool width is whatever `navigator.hardwareConcurrency` reported. `base` is advanced once
  per tick on the sequential path by `advance_tick_seed` — folding the tick in only through
  `chunk_seed` measured 14% slower on SIR with identical content, and that was never explained.
  Boids, ants, Virus on a Network and Team Assembly each have a
  `results_do_not_depend_on_the_thread_count` test, as do `cpu/grid_engine.rs`, `cpu/layout.rs`
  and `cpu/primitives/components.rs`. Keep them.
- `AgentModel::CHUNK` is per-model on purpose. It sets both the RNG seeding granularity and the
  parallel load balance, so it must be a fixed const (not derived from the thread count) but still
  small enough to split across every core — 4096 gave only 13 chunks for 50k boids and cost 20%.
  512 is the default, boids overrides to 64 (its kernel draws nothing, and 512 gave a thousand
  agents two chunks) and ants to 4096. Changing ants' changes its results, since a chunk is its
  seeding unit; changing boids' does not.
- `SpatialHash` (`henad-core/src/spatial_hash.rs`) is a flat counting-sort grid, rebuilt every
  tick from agent positions — this replaced a naive neighbor search and was the biggest lever in
  getting boids to scale. All neighbor queries (including toroidal wraparound) go through
  `query_radius` or `for_each_within`, which walk the same cells in the same order; the second
  hands a kernel the deltas the range test already computed, so it does not work them out twice.
  Don't reintroduce O(n²) neighbor search. `HashGrid::new` is the one place the cell geometry is
  decided, for the CPU sort and the GPU one alike, and it caps the grid at `MAX_INDEX_CELLS`.
- `henad-compute/src/cpu/primitives/scatter.rs` (`ScatterGrid`) handles the one write pattern the rest of the
  engine can't: many agents depositing into the same cell. Read its module docs before changing
  it — the strategy choice is measured (`benches/scatter.rs`), not assumed, and **atomics are not
  an option**: `fetch_max` scales negatively under contention (7.1 ms at one thread, 99.2 ms at
  four). Its three arms must stay bit-identical, because the arm is picked from the worker count
  and the deposit count, so any divergence would make a model's results depend on the machine.
  Re-run the bench rather than reasoning about it. The sparse arm (`Banded`) is what a field layer
  takes: measured 2.3x to 12.3x over the dense ones wherever there are more cells than deposits,
  and behind them at one deposit per cell even on one worker.
- Data layout is Struct-of-Arrays throughout (`pos_x: Vec<f32>`, `pos_y: Vec<f32>`, ... rather
  than `Vec<Agent>`) specifically for cache locality and rayon-friendliness — preserve this when
  adding fields to a model's state. `agent_lanes!` emits one `Vec<T>` per lane with named field
  access for exactly this reason.
- Benchmarking: this machine drifts up to 40% under sustained load, so only interleaved old-vs-new
  runs on a cooled machine mean anything. Game of Life is the cleanest signal, since it has no step
  RNG and its output is bit-identical across refactors. When reporting a number, say whether it is
  release-mode and what is actually being measured — a flat-out `step()` counter, not a frame rate.
  A surprisingly favourable result gets flagged as surprising rather than presented as a finding: a
  "300x GPU speedup" here was once a debug-build, framerate-capped artifact hiding a real 10x.

### GPU — traps that already cost real debugging

Each of these failed silently or misleadingly once. The engine now handles all of them, so the note
is about not undoing them.

- **A timestamp stamped on an empty compute pass is never written.** The symptom is a `start` of 0
  and an absurd elapsed time (an absolute GPU tick, ~4e14 ns). `gpu/agent_engine.rs` puts the
  opening stamp on the index rebuild's counting pass when there is an index, and on the first
  declared pass when there is not.
- **One oversized submission silently returns zeros.** Enough passes in a single command buffer
  trips the OS GPU watchdog — no error, no panic, and every later readback reads zero. Batch at 64
  steps per submission, as `GpuAgentState::run_batched` and the real runner do. This first showed up
  as a flaky test. The sweep's GPU tracks (`henad-explore/src/exec/gpu.rs`) keep the bound per run.
  Each command buffer holds the steps of one run, and two runs' buffers are never merged. N runs of
  64 steps in one buffer would trip the watchdog again.
- **`max_storage_buffers_per_shader_stage` is 8** in `wgpu::Limits::default()` and in the WebGPU
  baseline. `limits.rs::raise` asks for exactly what the models need, the `GpuNeeds` a host reads
  from its set through `ModelSet::gpu_needs()` before any device exists. Each GPU entry declares its
  own from its pass list — no constant, because wgpu's own advice is to request only what you need
  and a constant would be either short of a future model or dead headroom. For the example models
  it comes to 8, since `gpu_ants`'s step pass sits at exactly 8. `raise` takes the needs rather
  than knowing them: henad-compute cannot see which models a host offers.
  `every_gpu_model_builds_on_a_baseline_device` holds the line on a `Limits::default()` device, and
  asserts in the same breath that `capacity.rs` agrees — build and declared demand pin each other,
  so an over-reported pass count fails there.
  `every_gpu_entry_needs_the_bindings_its_widest_pass_binds` pins each entry's `GpuNeeds` to its
  demand. Note wgpu on Metal shares one argument table across storage + uniform + vertex, so a
  check counting only storage buffers can pass locally and fail there.
- **`Limits::default()` is not the hardware, and its _size_ limits are what bound a run.** The
  baseline caps one storage binding at 128 MiB, one buffer at 256 MiB and a texture side at 8192,
  where an M4 Pro offers 4 GiB, 14.3 GB and 16384. `limits.rs::raise` takes all three to whatever
  the adapter reports. Unlike the buffer count, these are deliberately machine-dependent: how big a
  run can be is a property of the hardware however we ask.
- **The display texture is a sampled view, never a mirror of the grid.** One texel per cell caps
  the grid at `max_texture_dimension_2d` and costs 4 bytes per cell, which at 16384² is 1.07 GB of
  RGBA for something drawn into a ~1000 px panel. `display_scale.rs` caps each axis at
  `MAX_DISPLAY_DIM`, a display pass dispatches per _texel_ and reads the cell at
  `texel * grid / tex`, and `viewport.rs` samples the CPU grid the same way on upload. Both are
  identity below the cap.
- **A model over the device's limit is refused before it is built.** `gpu/capacity.rs` computes a
  model's buffer sizes, texture dimensions and per-pass storage-binding counts from what it already
  declares, and checks them first. The app disables Build and both engines assert with a readable
  message. That covers sizes and binding counts. The cheap half.
- **Everything else the device rejects reaches the UI instead of the process.** wgpu's default handler panics on any
  error no scope claims. `gpu::fault::catching_on` wraps model construction in error scopes for all
  three `ErrorFilter`s, and `GpuContext::new` installs `on_uncaptured_error` as the floor under
  every path no scope covers, egui's own rendering included. Contracts `capacity.rs` cannot see
  (workgroup size, uniform layout, an allocation the device has no memory for) now reach the UI as
  a modal. Do not undo either half — remove the handler and the next validation error ends the
  process. Note error scopes are **thread-local**, so a scope pushed on the UI thread never sees
  what a sim thread does. The sink exists to cover that asymmetry.
- **A panicking kernel is caught too, and its location needs help.** Both sim threads wrap
  `run()` once at thread start, outside the loop, so the catch costs nothing per tick. Rayon
  catches a worker's panic and re-raises it on the caller with `resume_unwind`, which does not run
  the panic hook again, so `fault.rs` keeps a global fallback alongside its thread-local record.
  Drop the fallback and every `step_cell` panic loses its `file:line`.
- **Two clocks that must be reset together.** `gpu/sim_thread.rs` gates its stats refresh on
  `last_stats_publish` but divides by `tps_timer`; resetting one without the other reports a whole
  batch over a near-zero window as a plausible-looking TPS. Go through `reset_tps_window`.
- **A stats sample encoded while the previous readback is pending is dropped.** `CounterReadback`
  skips its copy while a map is in flight, and the stats repeat the older values with no error.
  `stepping::submit_slice` debug-asserts against it, and `stepping::sample_stats` collects a
  readback still in flight before it encodes its own. A poll that gives up a map unmaps the staging
  buffer. A buffer left mapped or pending makes the next copy into it a validation error and fails
  every later map. A host that pipelines its samples learns from the `StatsPoll` that
  `poll_stats_readback` returns whether the latest one landed or failed. The
  GPU tracks in `henad-explore/src/exec/gpu.rs` record no sample while one reads back, and assert
  it outside their error scopes, where a panic would pass for a failed run.
- **A lost device fails quietly first.** After `Device::destroy` or a driver loss, polls and waits
  still succeed, and builds and readbacks fail as ordinary validation errors. wgpu reports the loss
  only through the device-lost callback, and for a destroyed device only once a poll finds its
  queue empty. `GpuContext::new` installs that callback and `GpuContext::is_lost` reads it. The GPU
  tracks drain the device and check it before writing any run that failed on the GPU. Otherwise
  every run left is written as `gpu_error` instead of being left for `--resume`.
- **Uniform layouts and a model's binding slots are generated.** The `build.rs` of henad-compute,
  henad-models and henad-app runs henad-build's `ShaderBuild` over that crate's shaders, and
  `henad_compute::include_shaders!()` at the crate root brings the output in from `OUT_DIR` as
  `shader_bindings` and `binding_decls`, each under one allow list, `unsafe_code` included. The
  macro and the generated code name `include!`, `concat!`, `env!` and `assert!` through `::core`.
  henad-compute names its entry points with `ShaderBuild::new("src/gpu")`, and the other two use
  `discover` over `src` and `src/ui`: every `.wgsl` file without a `#define_import_path` line.
  `discover` refuses a path component that is no Rust identifier or is a keyword, names that
  collide, a `henad.wgsl` file or a `henad` directory holding a `.wgsl` file in any case, which
  would shadow a shared module, and a first component the generated code uses at its root (`wgpu`,
  `bytemuck`, `std`, `core`, `alloc`, `_root`, `ShaderEntry`, `layout_asserts`, `bytemuck_impls`). A
  module with a `@binding(` line fails the build. The binding constant is the path's components
  upper-cased and joined by `_`. `generate` copies the shared modules from henad-core into
  `OUT_DIR/henad_wgsl/henad/`, runs `wgsl_bindgen` (pinned to `=0.23.3`, and
  `scripts/check_packaging.sh` holds `WGSL_BINDGEN_VERSION` to the pin) with its own rerun lines
  off, prints one `cargo:rerun-if-changed` for the shader root (and one per explicit entry), in the
  single-colon form that keeps a downstream MSRV below 1.77, skips the pass when the FNV-1a stamp of
  its inputs and of `output.rs` itself matches `shader_bindings.stamp`, and writes each file only
  when its bytes change. A crate with no shaders gets an empty `shader_bindings.rs` and loses its
  stamp. Otherwise the shaders' return would match the old stamp and keep the empty file.
  `ShaderBuildError`'s `Debug` writes its `Display`. Uniform structs, workgroup sizes and bind group
  layouts therefore come from the WGSL, and each model asserts its own struct against the generated
  one. Shared WGSL is reached with `#import henad::<module>`, resolved at build time, so no shader
  is assembled at runtime any more. henad-build also reads each entry's `@group(0)` lines into
  `binding_decls`, in `@binding` order, and fails the build on any line holding `@binding(` in
  another form than `@group(G) @binding(N) var<...> name: Type;` or a gap in the indices. A
  compile-time assertion in `binding_decls.rs` holds each list to the length of the generated
  `WgpuBindGroup0` layout, and `include_shaders!` asserts that the shaders were composed against the
  `SHARED_WGSL_FNV1A64` of the henad-core it links. henad-build's tests run `generate` against a
  scratch `OUT_DIR`, never Cargo. The engine resolves each name itself. `params`, `dims`, `output`,
  `cell_start`, `sorted`, `counters` and `partials` are reserved, and any other name is a
  `BufferSpec` label with an optional `_in` or `_out` suffix. The access mode picks the side.
  `henad-core/src/authoring/model/binding.rs` is the reference. An imported constant or type reaches
  the generated bindings exactly when an entry point references it, since naga keeps only what an
  entry point references. Hence `grid_dims.wgsl` for `Dims`, and the `CODES` array in `parity.wgsl`
  for `MOORE_ROW_MAJOR` and the other codes.

### Sim runs off the UI thread

`SimThread` (`henad-compute/src/cpu/sim_thread.rs`) exists so simulation stepping never blocks
rendering. It is a thin handle over a `Driver<Loop>`, and the split underneath it lives in
`henad-compute/src/runner/`. A `SimLoop` does whatever is due now and says when it next wants
calling, as `Pace::Idle`, `Pace::Now` or `Pace::After`. A `Driver` decides how to wait.
`runner/thread.rs` runs the loop on an OS thread of its own, taking `mpsc` commands.
`runner/frame.rs` pumps it inline from `SimThread::update()`, which `henad-app` calls each frame,
and stops once a frame has spent `PUMP_BUDGET_MS`. `wasm32-unknown-unknown` cannot spawn a thread
even with atomics, hence the second driver.

The `#[cfg(target_arch = "wasm32")]` choosing between the two sits in `runner/mod.rs` and nowhere
else, so stepping is written once. `SimThread`'s API (`play`/`pause`/`step_once`/`run_to`/
`set_schedule`/`send`/`update`) is the same either way and `henad-app` never learns which driver it
holds. `gpu/sim_thread.rs` is driven the same way.

`SimCommand::SetSchedule` replaces the loop's `Schedule`, fires the entries due at the current tick
unless that tick has fired, and publishes. Both loops keep `fired_through`, the highest tick whose
actions have had their turn, set once a step reaches a tick (on the GPU, once a batch or
`StepOnce` passes it) or a schedule fires it. Only the first schedule sent before the first step
fires the build tick, and no tick fires twice. From then on each entry fires once, after the step
that reaches its tick.
This is `Fire::AfterStep`, the rule of `henad-cli --export-stats` and a sweep's `RunCursor`, and
the snapshot at tick t carries tick t's actions. An empty schedule costs one test per step, and
`Schedule::run_due` allocates nothing when no action is refused. `SimCommand::RunTo(tick)` steps
uncapped toward the tick and never past it, publishes every `runner::RUN_TO_PUBLISH_INTERVAL` on the
way, and at the tick pauses and force-publishes. `Play`, `Pause` and `StepOnce` cancel it. A tick
at or behind the current one pauses at once. Going back is the host's job, and the app rebuilds
first. The GPU loop cuts a batch at the target and a submission at each tick an action is due at,
gives each action a submission of its own (`encode_action` needs one per press), and at the target
takes the blocking `snapshot_now`. On native the stats then equal the state at the target. In a
browser the readback cannot block, and the stats land on a later pump. Either driver runs a run-to
through the same `pump`, and the frame driver cuts it at `PUMP_BUDGET_MS` like any other.
`a_replay_of_a_planned_run_matches_its_sweep_row` and `a_gpu_schedule_matches_export_stats`
(henad-explore) pin the live loop's rule to a sweep's and to `--export-stats`. Keep them.

A sweep started from the app runs off the UI thread as well, through
`henad_explore::handle::SweepRun`. On native the handle spawns a thread that runs the sweep as
`run_sweep` does (`run_in_memory` or `run_into_directory`), lanes or tracks and all, and its
`SweepControl` holds or ends every run between two slices of steps. In a browser it wraps a
`runner::Driver<PumpedSweep>`, and `HenadApp::logic` pumps it each frame through
`ui::sweep::update` and `SweepRun::update`. Either way the host reads `SweepEvent`s from an
`mpsc` channel that loses none (`Planned`, each `RunFinished` in plan order, a search's
`SearchBatchTold` with an `Arc<SearchUpdate>` after each batch's runs, then one `Finished` with the
`SweepRecord`, or `Failed`), and a `SweepProgress` where the latest write wins. The `wake`
callback of `SweepRunOptions` runs after each event, and an idle UI then repaints to collect it.
Dropping the handle aborts the sweep, and on native waits for the thread to write its files. A
pause never changes a result. A run's trajectory depends on its seed and schedule alone.

The UI thread only ever reads the latest snapshot (`snapshot.rs`) and never touches the live
`SimState` directly. Publishing goes through a `SnapshotSlot`, holding the `fresh` snapshot plus a
`spare` the host hands back for its buffers. Recycling is only an optimisation. Dropping a snapshot
instead means the next publish allocates.

`build_snapshot` calls `SimState::prepare_view` first, which is where a model turns state into
something drawable — ants quantises its `f32` pheromone field into palette indices there. That
runs on publish, not every tick, so anything a view needs but a step doesn't belongs in
`prepare_view` rather than in `step`.

A network model's layout relaxes next, in `SimState::relax_layout`, and stats come last. The layout
runs at least one iteration and keeps going until the budget from `SimCommand::SetLayout` is spent.
The budget is capped at `runner::MAX_VIEW_BUDGET_MS` on wasm, where the view is prepared inside the
frame pump. The layout moves nodes only on a publish that follows a tick, or on every publish while
paused if `while_paused` is set. An action publishes without a tick, and leaves a paused network
where it is unless `while_paused` is set. The layout never runs inside `step()`, so node positions
are not a function of the tick. The edge list is copied into the snapshot only when
`Network::version()` or the edge count changes. A recolour through `Network::update_colors` that
changes nothing leaves the version alone. The GPU runner accepts `SetLayout` and ignores it.
